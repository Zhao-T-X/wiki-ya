-- 0012 Run 级 token 账本（PR-07）。
--
-- 把每次 Run 的真实 token 用量（来自 provider 的 usage，非估算）落到统一
-- 登记处，使 Run Trace 能直接回答「这次运行花了多少 token / 多少钱」，
-- 而不必依赖各服务临时返回的 DTO。
--
-- 存 JSON（TokenUsage：input/output/embedding/retries），未发生调用或
-- provider 未上报时为 NULL——Trace 据此诚实显示「无用量记录」。

ALTER TABLE runs ADD COLUMN usage_json TEXT;
