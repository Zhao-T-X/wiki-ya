//! Domain 公共基础设施。
//!
//! 只放"与业务无关、但所有子域都要用"的东西：受控词表机制与标识类型。
//! 与业务有关的一切状态枚举都放在各自子域里，**不要**往这里堆大杂烩。

pub mod enums;
pub mod ids;

/// 时间戳统一表示。
///
/// 刻意用 `String` 而不是 `DateTime<Utc>`：SQLite 的 `datetime('now')`
/// 产出 `YYYY-MM-DD HH:MM:SS`（UTC），把它解析成 `DateTime` 再序列化回去
/// 只会引入格式漂移。领域层只做**比较与排序**（字符串在 UTC 定长格式下
/// 的字典序等价于时间序），格式化交给前端。
///
/// 时间语义的进一步约定（时区/精度）见
/// `docs/领域枚举与不变量定义.md` 的决策 D2。
pub type Timestamp = String;
