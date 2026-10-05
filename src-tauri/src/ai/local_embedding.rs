//! 本地向量化（fastembed / ONNX Runtime）。
//!
//! 背景：语义检索原先只能走远程 `/embeddings`，没配 Key 就完全没有语义能力。
//! 本模块让 `ai.embedding_model` 支持 `local:<模型名>` 前缀，把向量化搬到本机 ONNX
//! 推理：首次使用自动下载权重，之后**完全离线**，既不联网也不产生 API 费用。
//!
//! 三条硬约束（与项目既有原则一致）：
//!
//! 1. **诚实**：本地推理没有可上报的 token 用量（fastembed 不返回 token 数），
//!    因此 [`EmbeddingOutput::prompt_tokens`] 记 0，而不是拿字符数去"估算"一个
//!    看起来像真的数字。成本估算表里也没有本地模型的价格，故费用为 0。
//! 2. **不静默截断**：切段用的上下文上限来自模型登记表 [`LocalModel::max_tokens`]，
//!    而不再是硬编码的常量；换模型时上限随模型走。
//! 3. **不静默降维/串空间**：维度与登记表不符即报错，宁可不用语义，
//!    也不把不同模型的向量混存到同一个 model 键下。
//!
//! 缓存位置：`<app_data_dir>/models`（由 `lib.rs` 启动时注入；MCP 等无 Tauri
//! 上下文的二进制退回 `WIKIYA_MODEL_DIR` / 数据库同目录 / 临时目录）。
//! 注意 `HF_HOME` 的优先级**高于**本模块的目录设置（fastembed 的既定行为）。

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

use crate::ai::provider::EmbeddingOutput;
use crate::error::{AppError, AppResult};

/// `ai.embedding_model` 中标记"走本地推理"的前缀。
pub const LOCAL_PREFIX: &str = "local:";

/// 远程模型上下文未知时的保守上限（token）。
///
/// 远程端点的真实上限因服务商而异（`text-embedding-3-*` 是 8191，
/// 私有部署可能更小），本项目拿不到元数据，故沿用 bge 的 512 减掉
/// `[CLS]`/`[SEP]` 两个位置——宁可多切几段，也不静默截断。
const REMOTE_FALLBACK_CONTEXT_TOKENS: usize = 510;

/// 本地可用的嵌入模型登记表。
///
/// 只收**确认过** fastembed 支持、且本项目用得上的模型；不在表里的名字一律
/// 报错并列出可用项，绝不"猜一个最接近的"——猜错会让向量悄悄来自另一个模型。
#[derive(Debug)]
struct LocalModel {
    /// 设置里写的名字（`local:` 之后的部分）。
    name: &'static str,
    model: EmbeddingModel,
    /// 向量维度，用于校验实际输出，防止张冠李戴。
    dim: usize,
    /// 上下文上限（含 `[CLS]`/`[SEP]`）。
    max_tokens: usize,
    /// 权重来源（HF 仓库），仅用于日志与错误提示。
    repo: &'static str,
}

const LOCAL_MODELS: &[LocalModel] = &[
    LocalModel {
        name: "bge-small-zh-v1.5",
        model: EmbeddingModel::BGESmallZHV15,
        dim: 512,
        max_tokens: 512,
        repo: "Xenova/bge-small-zh-v1.5",
    },
    LocalModel {
        name: "bge-large-zh-v1.5",
        model: EmbeddingModel::BGELargeZHV15,
        dim: 1024,
        max_tokens: 512,
        repo: "Xenova/bge-large-zh-v1.5",
    },
    LocalModel {
        name: "bge-small-en-v1.5",
        model: EmbeddingModel::BGESmallENV15,
        dim: 384,
        max_tokens: 512,
        repo: "Xenova/bge-small-en-v1.5",
    },
    LocalModel {
        name: "bge-base-en-v1.5",
        model: EmbeddingModel::BGEBaseENV15,
        dim: 768,
        max_tokens: 512,
        repo: "Xenova/bge-base-en-v1.5",
    },
    LocalModel {
        name: "bge-large-en-v1.5",
        model: EmbeddingModel::BGELargeENV15,
        dim: 1024,
        max_tokens: 512,
        repo: "Xenova/bge-large-en-v1.5",
    },
];

/// 单批推理条数上限。
///
/// ONNX Runtime 的算子调度对批量大小敏感，批量过大只是白烧内存；
/// 32 条 × 512 token 在本机实测是吞吐与显存的平衡点。
const INFER_BATCH: usize = 32;

/// 模型缓存根目录（`lib.rs` 启动时注入）。
static CACHE_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// 已加载的本地模型。`TextEmbedding::embed` 需要 `&mut self`，故用互斥量串行化；
/// 本地推理本身是 CPU 密集型，串行反而比多线程互相抢核更稳。
struct Loaded {
    model: &'static LocalModel,
    inner: TextEmbedding,
}

static LOADED: OnceLock<Mutex<Option<Loaded>>> = OnceLock::new();

/// 注入模型缓存根目录（`<app_data_dir>`）。只能生效一次，重复调用以首次为准。
pub fn set_cache_root(dir: PathBuf) {
    if let Err(dir) = CACHE_ROOT.set(dir) {
        crate::log_warn!("本地模型缓存目录已初始化，忽略本次设置：{}", dir.display());
    }
}

/// 模型缓存目录。
///
/// 打包后进程的 CWD 不可控（可能为 `/`），所以绝不把权重落在相对路径下。
fn cache_dir() -> PathBuf {
    if let Some(root) = CACHE_ROOT.get() {
        return root.join("models");
    }
    // 无 Tauri 上下文的入口（如独立 MCP server）按环境变量 / 数据库同目录找。
    if let Ok(dir) = std::env::var("WIKIYA_MODEL_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    if let Ok(db_path) = std::env::var("WIKIYA_DB_PATH") {
        if let Some(parent) = std::path::Path::new(&db_path).parent() {
            if !parent.as_os_str().is_empty() {
                return parent.join("models");
            }
        }
    }
    crate::log_warn!("未注入模型缓存目录，退回临时目录（重启后可能需要重新下载权重）");
    std::env::temp_dir().join("wiki-ya-models")
}

/// 该 `ai.embedding_model` 取值是否要求走本地推理。
pub fn is_local_spec(spec: &str) -> bool {
    spec.trim().starts_with(LOCAL_PREFIX)
}

/// 剥掉 `local:` 前缀，取出模型名；非本地取值返回 `None`。
pub fn parse_local_model(spec: &str) -> Option<&str> {
    spec.trim().strip_prefix(LOCAL_PREFIX).map(str::trim)
}

/// 可用本地模型名（供设置页提示与错误信息）。
pub fn supported_models() -> Vec<&'static str> {
    LOCAL_MODELS.iter().map(|m| m.name).collect()
}

/// 单个本地模型的对外元数据（设置页据此渲染，不在前端硬编码任何数字）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelInfo {
    /// 模型名（`local:` 之后的部分）。
    pub name: &'static str,
    /// 可直接填进「向量模型」输入框的完整取值。
    pub spec: String,
    pub dimensions: usize,
    /// 上下文上限（含 `[CLS]`/`[SEP]`）。
    pub max_tokens: usize,
    /// 权重来源（HF 仓库）。
    pub repo: &'static str,
    /// 权重是否已在本机缓存目录里。
    pub downloaded: bool,
    /// 已下载权重的磁盘占用（字节）；未下载为 0。
    pub weight_bytes: u64,
    /// 下载量级的人类可读提示（如 `约 91 MB`）；未下载时为 None。
    pub download_hint: Option<String>,
}

/// 列出全部本地模型及其缓存状态。
///
/// 「是否已下载」是设置页最需要的信息：没下载的模型首次使用要联网拉 90MB，
/// 用户点下去之前就该知道，而不是等检索时卡住。
pub fn list_models() -> Vec<LocalModelInfo> {
    LOCAL_MODELS
        .iter()
        .map(|model| {
            let (downloaded, weight_bytes) = cache_weight_size(model.repo);
            LocalModelInfo {
                name: model.name,
                spec: format!("{LOCAL_PREFIX}{}", model.name),
                dimensions: model.dim,
                max_tokens: model.max_tokens,
                repo: model.repo,
                downloaded,
                weight_bytes,
                download_hint: (!downloaded).then(|| weight_hint(model)),
            }
        })
        .collect()
}

/// 估算某模型的下载量级（只用于提示，不是承诺）。
///
/// 数值来自 ONNX 权重的典型体积量级，允许偏差——文案里会写"约"。
fn weight_hint(model: &LocalModel) -> String {
    let mb = model.dim as u64 * 4 / 1024; // 粗略：参数量 ≈ 维度 × 4
    let size = mb.clamp(20, 600);
    format!("约 {size} MB")
}

/// HF 缓存里的仓库目录名：`Xenova/bge-small-zh-v1.5` → `models--Xenova--bge-small-zh-v1.5`。
///
/// **只把 `/` 换成 `--`，点号必须原样保留。** 早期版本顺手把 `.` 也替换了，
/// 结果永远匹配不到已下载的权重，设置页把下好的模型显示成"需下载 91MB"。
fn hf_cache_dir_name(repo: &str) -> String {
    format!("models--{}", repo.replace('/', "--"))
}

/// 查某个 HF 仓库在缓存目录里的实际占用与是否完整。
fn cache_weight_size(repo: &str) -> (bool, u64) {
    let dir = cache_dir().join(hf_cache_dir_name(repo));
    let snapshots = dir.join("snapshots");
    if !snapshots.is_dir() {
        return (false, 0);
    }
    // snapshots 下是符号链接（指回 blobs），递归统计**实体文件**大小，
    // 直接累加 snapshots 会把同一份权重数两次（不对）。
    let blobs = dir.join("blobs");
    if !blobs.is_dir() {
        return (false, 0);
    }
    let mut total = 0u64;
    let mut files = 0usize;
    for entry in std::fs::read_dir(&blobs).into_iter().flatten().flatten() {
        if entry.path().extension().map(|e| e == "lock").unwrap_or(false) {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            if meta.is_file() {
                total += meta.len();
                files += 1;
            }
        }
    }
    // 至少要有权重文件才算就绪；只有 .lock 说明下载没完成。
    (files > 0, total)
}

/// 把设置里的取值解析成登记表项。
fn resolve(spec: &str) -> AppResult<&'static LocalModel> {
    let name = parse_local_model(spec).ok_or_else(|| {
        AppError::Invalid(format!(
            "本地向量化需要 `{LOCAL_PREFIX}` 前缀，当前取值是 `{}`",
            spec.trim()
        ))
    })?;
    LOCAL_MODELS
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| {
            AppError::Invalid(format!(
                "未知的本地嵌入模型 `{name}`。可用：{}",
                supported_models()
                    .iter()
                    .map(|n| format!("{LOCAL_PREFIX}{n}"))
                    .collect::<Vec<_>>()
                    .join("、")
            ))
        })
}

/// 切段用的上下文上限（token）。
///
/// 本地模型取登记表里的真实上限减 2（`[CLS]`/`[SEP]`）；远程模型拿不到元数据，
/// 保守回退到 510（见 [`REMOTE_FALLBACK_CONTEXT_TOKENS`]）。
pub fn embedding_context_tokens(spec: &str) -> usize {
    if !is_local_spec(spec) {
        return REMOTE_FALLBACK_CONTEXT_TOKENS;
    }
    match parse_local_model(spec).and_then(|name| {
        LOCAL_MODELS
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
    }) {
        Some(model) => model.max_tokens.saturating_sub(2),
        // 未登记的名字：不在这里报错（切段只是估算），
        // 真正的硬校验发生在 [`embed`] 加载模型时。
        None => REMOTE_FALLBACK_CONTEXT_TOKENS,
    }
}

/// 预热：下载并加载模型。启动时后台调用，让首次检索不必等下载。
pub fn warmup(spec: &str) -> AppResult<()> {
    if !is_local_spec(spec) {
        return Ok(());
    }
    let name = parse_local_model(spec).unwrap_or_default().to_string();
    match with_model(spec, |_| Ok(())) {
        Ok(()) => {
            crate::log_info!("本地嵌入模型已就绪：{name}");
            Ok(())
        }
        Err(err) => {
            crate::log_error!("本地嵌入模型预热失败（{name}）：{err}");
            Err(err)
        }
    }
}

/// 本地向量化入口。
///
/// 与 `Provider::embed` 同签名，便于在 provider 层直接替换；
/// `spec` 必须是 `local:<模型名>` 形式的完整取值。
pub fn embed(spec: &str, texts: &[String]) -> AppResult<EmbeddingOutput> {
    // 空输入必须早退：否则会为了 0 条文本把整个模型加载起来。
    if texts.is_empty() {
        return Ok(EmbeddingOutput {
            vectors: Vec::new(),
            prompt_tokens: 0,
        });
    }
    let expected = resolve(spec)?;
    let wanted = texts.len();

    let raw = with_model(spec, |inner| {
        inner
            .embed(texts, Some(INFER_BATCH))
            .map_err(|err| AppError::Internal(format!("本地向量化失败：{err}")))
    })?;

    // 与远程路径同样的诚实校验：数量对不上说明推理层不可信，宁可报错不用。
    if raw.len() != wanted {
        return Err(AppError::Internal(format!(
            "本地向量化数量不符：请求 {wanted} 条，返回 {} 条",
            raw.len()
        )));
    }

    let mut vectors = Vec::with_capacity(wanted);
    for vector in raw {
        if vector.len() != expected.dim {
            return Err(AppError::Internal(format!(
                "本地嵌入维度不符：{} 期望 {} 维，实际 {} 维",
                expected.name,
                expected.dim,
                vector.len()
            )));
        }
        vectors.push(l2_normalize(vector));
    }

    // 诚实性：本地推理没有计费，fastembed 也不返回 token 数，故记 0。
    Ok(EmbeddingOutput {
        vectors,
        prompt_tokens: 0,
    })
}

/// 取出（必要时加载）模型并在其上执行 `action`；模型名变化时重新加载。
fn with_model<T>(
    spec: &str,
    action: impl FnOnce(&mut TextEmbedding) -> AppResult<T>,
) -> AppResult<T> {
    let wanted = resolve(spec)?;
    let mut guard = LOADED
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| AppError::Internal("本地嵌入模型的互斥量已中毒".into()))?;

    let stale = guard
        .as_ref()
        .map(|loaded| !std::ptr::eq(loaded.model, wanted))
        .unwrap_or(true);
    if stale {
        let inner = load(wanted)?;
        *guard = Some(Loaded {
            model: wanted,
            inner,
        });
    }

    let loaded = guard
        .as_mut()
        .ok_or_else(|| AppError::Internal("本地嵌入模型加载后丢失".into()))?;
    action(&mut loaded.inner)
}

/// 下载（首次）并初始化模型会话。
fn load(model: &'static LocalModel) -> AppResult<TextEmbedding> {
    let dir = cache_dir();
    std::fs::create_dir_all(&dir)?;
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    crate::log_info!(
        "初始化本地嵌入模型 {}（{}，{} 维，上限 {} token，{} 线程）→ {}",
        model.name,
        model.repo,
        model.dim,
        model.max_tokens,
        threads,
        dir.display()
    );

    let options = TextInitOptions::new(model.model.clone())
        .with_cache_dir(dir)
        .with_max_length(model.max_tokens)
        .with_intra_threads(threads)
        // 桌面应用没有可刷新的进度条，indicatif 只会往看不见的 stdout 写；
        // 下载起止由外层日志交代。
        .with_show_download_progress(false);

    TextEmbedding::try_new(options).map_err(|err| {
        AppError::Internal(format!(
            "加载本地嵌入模型 {} 失败：{err}（首次使用需联网下载权重；\
             国内网络可设置 HF_ENDPOINT=https://hf-mirror.com 后重试）",
            model.name
        ))
    })
}

/// 就地 L2 归一化。
///
/// BGE 系列要求归一化后用余弦相似度；虽然本库检索时算的就是余弦
/// （归一化不改变排序），但归一化后余弦退化为点积，既省掉每次比较的
/// `sqrt`，也避免不同段落模长差异大时的浮点抵消。
///
/// 全零向量保持全零：不能除以 0 产出 NaN——NaN 会静默污染排序，
/// 比"这条没有相似度"更难发现。
fn l2_normalize(mut vector: Vec<f32>) -> Vec<f32> {
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if !norm.is_finite() || norm <= f32::EPSILON {
        return vector;
    }
    for value in vector.iter_mut() {
        *value /= norm;
    }
    vector
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_prefix_is_detected_after_trim() {
        assert!(is_local_spec("local:bge-small-zh-v1.5"));
        assert!(is_local_spec("  local:bge-small-zh-v1.5  "));
        assert!(!is_local_spec("text-embedding-3-small"));
        assert!(!is_local_spec("bge-small-zh-v1.5"));
    }

    #[test]
    fn parses_model_name_case_insensitively() {
        assert_eq!(
            parse_local_model("local:bge-small-zh-v1.5"),
            Some("bge-small-zh-v1.5")
        );
        assert_eq!(
            parse_local_model(" local:BGE-Small-ZH-V1.5 "),
            Some("BGE-Small-ZH-V1.5")
        );
        assert_eq!(parse_local_model("text-embedding-3-small"), None);
    }

    /// 未登记的模型名必须报错并给出可用清单——绝不能猜一个相近的模型。
    #[test]
    fn unknown_model_is_rejected_with_available_list() {
        let err = resolve("local:bge-medium-zh-v1.5").unwrap_err();
        let message = err.to_string();
        assert!(message.contains("bge-medium-zh-v1.5"), "{message}");
        assert!(message.contains("local:bge-small-zh-v1.5"), "{message}");
    }

    #[test]
    fn missing_prefix_is_rejected() {
        let err = resolve("bge-small-zh-v1.5").unwrap_err();
        assert!(err.to_string().contains("local:"), "{err}");
    }

    /// bge-small-zh-v1.5 是 512 上限，切段应拿 510（留 [CLS]/[SEP]）。
    /// 这与改造前的硬编码值一致，故本 PR 不改变既有切段行为。
    #[test]
    fn context_tokens_come_from_the_model_registry() {
        assert_eq!(embedding_context_tokens("local:bge-small-zh-v1.5"), 510);
        // 远程模型仍走保守回退，行为不变。
        assert_eq!(embedding_context_tokens("text-embedding-3-small"), 510);
        // 未登记的本地名不猜，回到保守值；真正的硬校验在 embed()。
        assert_eq!(embedding_context_tokens("local:nope"), 510);
    }

    #[test]
    fn normalize_produces_unit_length() {
        let vector = l2_normalize(vec![3.0, 4.0]);
        let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6, "{norm}");
        assert_eq!(vector, vec![0.6, 0.8]);
    }

    /// 全零向量绝不能变 NaN：NaN 会静默污染相似度排序。
    #[test]
    fn normalize_keeps_zero_vector_intact() {
        assert_eq!(l2_normalize(vec![0.0, 0.0]), vec![0.0, 0.0]);
        assert_eq!(l2_normalize(Vec::new()), Vec::<f32>::new());
    }

    /// 空输入必须早退，否则单元测试会为了 0 条文本去下载上百 MB 权重。
    #[test]
    fn empty_input_never_loads_the_model() {
        let output = embed("local:bge-small-zh-v1.5", &[]).unwrap();
        assert!(output.vectors.is_empty());
        // 本地推理无可上报的 token 用量。
        assert_eq!(output.prompt_tokens, 0);
    }

    /// 非本地取值不得被当成"名字恰好叫 local:"之外的模型去猜。
    #[test]
    fn embed_rejects_non_local_spec() {
        assert!(embed("text-embedding-3-small", &["x".to_string()]).is_err());
    }

    /// 设置页元数据：不能有硬编码数字错位（维度/上限必须与登记表一致）。
    #[test]
    fn model_listing_matches_the_registry() {
        let list = list_models();
        assert_eq!(list.len(), LOCAL_MODELS.len());
        for info in &list {
            let model = LOCAL_MODELS
                .iter()
                .find(|m| m.name == info.name)
                .expect("列表项必须来自登记表");
            assert_eq!(info.dimensions, model.dim);
            assert_eq!(info.max_tokens, model.max_tokens);
            assert_eq!(info.spec, format!("{LOCAL_PREFIX}{}", model.name));
            // 没下载时不能谎报已就绪。
            assert!(
                info.downloaded || info.weight_bytes == 0,
                "{} 未下载却报告了占用",
                info.name
            );
        }
    }

    /// HF 缓存目录名推导：点号**必须保留**。
    ///
    /// 早期版本把 `.` 也换成 `--`，于是永远匹配不到已下载的权重，
    /// 设置页把下好的模型显示成"需下载 91MB"。
    #[test]
    fn hf_cache_dir_name_keeps_dots() {
        assert_eq!(
            hf_cache_dir_name("Xenova/bge-small-zh-v1.5"),
            "models--Xenova--bge-small-zh-v1.5"
        );
        // 每个登记在册的模型都能推出一个含点号的目录名（否则界面的"已下载"永不亮）。
        for model in LOCAL_MODELS {
            assert!(
                hf_cache_dir_name(model.repo).ends_with(model.name),
                "{} → {} 应以模型名结尾",
                model.repo,
                hf_cache_dir_name(model.repo)
            );
        }
    }

    /// 没有权重时必须诚实报 0，不能猜一个数字冒充已下载。
    #[test]
    fn missing_cache_reports_not_downloaded() {
        let (downloaded, bytes) = cache_weight_size("Xenova/definitely-not-a-real-model-xyz");
        assert!(!downloaded);
        assert_eq!(bytes, 0);
    }

    /// 真实推理冒烟：会联网下载权重（约 90MB）并跑 ONNX，故默认忽略。
    ///
    /// ```bash
    /// WIKIYA_MODEL_DIR=/tmp/wiki-ya-models cargo test --lib local_embedding -- --ignored --nocapture
    /// ```
    ///
    /// 断言的是**语义**而不是"没报错"：同义句必须比无关句更接近。
    /// 只断言维度的话，一个恒输出固定向量的模型也能通过。
    #[test]
    #[ignore = "需要下载模型权重并执行 ONNX 推理，仅手动运行"]
    fn local_embedding_smoke() {
        let output = embed(
            "local:bge-small-zh-v1.5",
            &[
                "如何配置向量化的模型？".to_string(),
                "向量模型要怎么设置？".to_string(),
                "今天中午吃什么比较好？".to_string(),
            ],
        )
        .expect("本地向量化应成功");

        assert_eq!(output.vectors.len(), 3);
        assert!(
            output.vectors.iter().all(|v| v.len() == 512),
            "bge-small-zh-v1.5 应为 512 维"
        );
        // 本地推理无可上报的 token 用量。
        assert_eq!(output.prompt_tokens, 0);

        let dot = |a: &[f32], b: &[f32]| {
            a.iter()
                .zip(b.iter())
                .map(|(x, y)| x * y)
                .sum::<f32>()
        };
        let related = dot(&output.vectors[0], &output.vectors[1]);
        let unrelated = dot(&output.vectors[0], &output.vectors[2]);
        assert!(
            related > unrelated,
            "同义句余弦({related}) 应大于无关句({unrelated})"
        );
        println!("同义句={related:.4} 无关句={unrelated:.4}");
    }
}
