//! wiki-ya MCP Server（Phase 7，TDD §55）。
//!
//! 独立二进制：stdio 上的 JSON-RPC 2.0（MCP 协议），把 wiki-ya 的知识能力
//! 以工具形式暴露给外部 Agent（如 Claude Desktop）。
//!
//! 只暴露只读工具（TDD §50/§51）：Agent → Application Service → Domain，
//! 永不直接执行 SQL；写路径只存在于桌面应用内的人审流程。
//!
//! 运行方式：
//!
//! ```text
//! WIKIYA_DB_PATH=/path/to/wikiya.db wikiya-mcp
//! ```
//!
//! 未设置 `WIKIYA_DB_PATH` 时默认使用当前目录的 `wikiya.db`。

use std::io::{BufRead, Write};

use serde_json::{json, Value};

use wiki_ya_lib::application::knowledge_service;
use wiki_ya_lib::application::search_service;
use wiki_ya_lib::application::settings_service;
use wiki_ya_lib::application::dto::SearchInput;
use wiki_ya_lib::domain::common::ids::{ClaimId, EntityId};
use wiki_ya_lib::infrastructure::db;

fn main() {
    let db_path = std::env::var("WIKIYA_DB_PATH").unwrap_or_else(|_| "wikiya.db".to_string());

    // 启动即建表 + 迁移（幂等），与桌面应用一致。
    if let Err(err) = db::initialize(std::path::Path::new(&db_path)) {
        eprintln!("wikiya-mcp：数据库初始化失败：{err}");
        std::process::exit(1);
    }

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue; // 忽略无法解析的行（诚实：不猜语义）
        };

        let id = request.get("id").cloned();
        let method = request
            .get("method")
            .and_then(|value| value.as_str())
            .unwrap_or("");

        // notification（无 id）不需要响应。
        let Some(id) = id else { continue };

        let response = match method {
            "initialize" => json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "wiki-ya", "version": env!("CARGO_PKG_VERSION") },
            }),
            "ping" => json!({}),
            "tools/list" => json!({ "tools": tools_list() }),
            "tools/call" => match tools_call(&db_path, request.get("params").unwrap_or(&Value::Null)) {
                Ok(result) => result,
                Err(err) => json!({
                    "content": [{ "type": "text", "text": err.to_string() }],
                    "isError": true,
                }),
            },
            other => json!({
                "content": [{ "type": "text", "text": format!("未知方法 `{other}`") }],
                "isError": true,
            }),
        };

        let output = json!({ "jsonrpc": "2.0", "id": id, "result": response });
        if writeln!(stdout, "{output}").is_err() {
            break; // 客户端断开
        }
        let _ = stdout.flush();
    }
}

/// 工具清单（只读白名单，与桌面应用内 Agent 工具同源语义）。
fn tools_list() -> Vec<Value> {
    vec![
        tool(
            "search_knowledge",
            "在 wiki-ya 知识库中检索文档 / Claim / 实体",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "default": 8 },
                },
                "required": ["query"],
            }),
        ),
        tool(
            "get_entity",
            "读取实体详情（别名 / Claims / 关系）",
            json!({
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"],
            }),
        ),
        tool(
            "get_claim",
            "读取 Claim 详情（含证据）",
            json!({
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"],
            }),
        ),
        tool(
            "get_evidence",
            "读取某条 Claim 的全部证据",
            json!({
                "type": "object",
                "properties": { "claim_id": { "type": "string" } },
                "required": ["claim_id"],
            }),
        ),
        tool(
            "knowledge_health",
            "读取知识库健康指标（全部真实计数）",
            json!({ "type": "object", "properties": {} }),
        ),
    ]
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
    })
}

/// 执行一次工具调用；所有失败都以 `isError: true` 诚实返回，绝不伪造。
fn tools_call(db_path: &str, params: &Value) -> Result<Value, wiki_ya_lib::error::AppError> {
    let name = params
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));

    let conn = db::open(std::path::Path::new(db_path))?;

    fn text(value: &impl serde::Serialize) -> Value {
        let body = serde_json::to_string_pretty(value).unwrap_or_default();
        json!({ "content": [{ "type": "text", "text": body }] })
    }

    match name {
        "search_knowledge" => {
            let query = args
                .get("query")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            let limit = args
                .get("limit")
                .and_then(|value| value.as_u64())
                .map(|value| value as usize)
                .unwrap_or(8);
            let response = search_service::search(
                &conn,
                SearchInput {
                    query,
                    limit: Some(limit),
                    semantic: Some(false),
                    kinds: None,
                },
            )?;
            Ok(text(&response))
        }
        "get_entity" => {
            let id = str_arg(&args, "id")?;
            let detail = knowledge_service::get_entity(&conn, &EntityId::from_raw(id), 1)?;
            Ok(text(&detail))
        }
        "get_claim" => {
            let id = str_arg(&args, "id")?;
            let detail = knowledge_service::get_claim(&conn, &ClaimId::from_raw(id))?;
            Ok(text(&detail))
        }
        "get_evidence" => {
            let id = str_arg(&args, "claim_id")?;
            let cards = knowledge_service::list_evidence(&conn, &ClaimId::from_raw(id))?;
            Ok(text(&cards))
        }
        "knowledge_health" => {
            let report = settings_service::knowledge_health(&conn)?;
            Ok(text(&report))
        }
        other => Err(wiki_ya_lib::error::AppError::Internal(format!(
            "未知工具 `{other}`（只读白名单之外一律拒绝）"
        ))),
    }
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, wiki_ya_lib::error::AppError> {
    args.get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            wiki_ya_lib::error::AppError::Internal(format!("工具参数 `{key}` 缺失"))
        })
}
