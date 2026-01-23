# 数据完整性校验与自动补拉——设计文档

## 实现的功能
1. 启动时校验与周期校验的统一调度框架
2. 自动补拉与修复验证流程
3. 全局健康状态管理与只读接口暴露
4. 指标、日志、告警的观测与接入
5. 可插拔的校验策略注册表

## 所有的技术
- **异步框架**：Tokio + Actix（Actor 消息传递、定时任务）
- **HTTP/WS 拉取**：复用 yue 层既有 REST 和 WebSocket 封装（无直接调用）
- **数据存储**：复��� yu/li 层 DuckDB/RocksDB 访问封装（无直接调用）
- **配置管理**：基于 config crate，支持环境变量覆盖（YU_* 前缀）
- **观测**：指标通过日志和可选的指标收集器（复用既有集成），告警通过 li::notification
- **并发控制**：信号量/受限队列限流，指数退避策略

## 流程图（含数据流向）
```
                ┌────────────────────┐
                │ 应用启动           │
                └───────┬───────────┘
                        │
                        ▼
            ┌────────────────────────┐
            │ DataIntegritySupervisor│
            └──────────┬────────────┘
                       │
                       │加载配置 + 初始化 HealthState=OK
                       │即刻触发首轮校验 + 启动周期定时
                       ▼
            ┌────────────────────────┐
            │  DataIntegrityChecker  │
            │  (单一 Actor)          │
            └───────┬───────────┬────┘
                    │           │
    启动触发首轮校验 │           │周期 interval 触发校验
                    ▼           ▼
                 ┌───────────────────┐
                 │  ValidationResult │
                 │  (缺口事件)       │
                 └────────┬──────────┘
                          │
                          ▼
                 ┌───────────────────┐
                 │   RepairExecutor  │
                 │  (限流+队列)      │
                 └───┬─────────┬────┘
                     │补拉/重算 │失败
                     ▼         ▼
         ┌──────────────────┐  │
         │ 修复验证校验     │  │
         │ (轻量)           │  │
         └───────┬──────────┘  │
                 │成功          │失败/超限
                 ▼             ▼
      ┌──────────────────┐  ┌─────────────────┐
      │ Supervisor更新   │  │ Supervisor更新  │
      │ HealthState=OK   │  │ HealthState=DEG │
      │ + 日志/指标      │  │ + 告警/日志     │
      └──────────────────┘  └─────────────────┘
                │                    │
                └────────┬───────────┘
                         │
                         ▼
                  ┌─────────────────┐
                  │ 外部只读查询    │
                  │ get_health_state│
                  └─────────────────┘
```

## 风险点
1. **启动超时**
   - 风险：启动校验耗时过长导致应用启动阻塞
   - 缓解：配置启动校验超时阈值，超时可降级标记 DEGRADED 但继续启动
   
2. **补拉风暴**
   - 风险：多个校验缺口同时触发补拉，对上游 API 造成压力
   - 缓解：RepairExecutor 加入并发上限、限流队列、指数退避
   
3. **状态不一致**
   - 风险：多路校验和修复并发时状态更新产生竞态
   - 缓解：状态唯一写入口在 Supervisor，所有更新都过 Actor Handler，保证原子性
   
4. **扩展点遗漏**
   - 风险：后续加入新的校验策略时需要修改框架代码
   - 缓解：预留策略注册表接口，新策略只需实现 trait 并注册，无需改框架
   
5. **配置错误**
   - 风险：环境变量配置错误导致校验频率过高或过低
   - 缓解：提供合理的默认值，配置加载时输出日志，告警异常值

## 设计的模块和组件
- **yu::data_integrity::supervisor**
  - DataIntegritySupervisor Actor：生命周期管理，启动 Checker，更新 HealthState
  - HealthState：状态枚举，含时间戳和原因说明
  - 暴露只读查询接口：GetHealthState Handler

- **yu::data_integrity::checker**
  - DataIntegrityChecker Actor：启动即首轮校验，后续按 interval 定时校验
  - 调用可插拔的 ValidationStrategy 执行具体校验
  - 产出 ValidationResult 事件，投递给 RepairExecutor

- **yu::data_integrity::repair**
  - RepairExecutor Actor：消费 ValidationResult，执行补拉/重算
  - 并发控制与指数退避管理
  - 补拉后触发轻量修复验证，上报修复结果给 Supervisor

- **yu::data_integrity::models**
  - ValidationResult：校验缺口事件结构
  - RepairRequest / RepairResult：补拉请求与结果
  - StrategyRegistry：校验策略注册表接口

- **yu::data_integrity::config**
  - DataIntegrityConfig：配置结构，支持 YU_* 环境覆盖
  - 配置项：startup_check_timeout_ms、periodic_check_interval_ms、periodic_check_window、repair_backoff

## 备选方案
1. **两个独立 Checker（vs 单一 Checker）**
   - 原方案：单一 Checker，启动时首轮 + interval 循环
   - 备选：StartupChecker + PeriodicChecker 两路独立
   - 选择单一原因：简化设计，减少组件数，两路校验逻辑相同且并发独立即可

2. **状态管理（vs 分布式一致性）**
   - 原方案：单机 Supervisor 原子更新状态，外部只读
   - 备选：Redis/Zookeeper 分布式状态
   - 选择单机原因：当前无多实例场景，复杂度不值得

3. **补拉触发（vs 人工审核）**
   - 原方案：自动补拉优先，失败后转人工
   - 备选：人工审核后再补拉
   - 选择自动原因：需求明确要求自动补拉优先

## 可能的扩展
1. **多实例部署**
   - 当前设计面向单实例
   - 扩展方向：加入分布式锁机制，协调多实例校验避免重复，或采用主从模式

2. **细粒度修复策略**
   - 当前统一补拉/重算
   - 扩展方向：按数据域定制修复动作（如优先本地重算 vs 远端补拉），通过策略注册表扩展

3. **校验规则多样化**
   - 当前框架与具体规则解耦
   - 扩展方向：当需要新的校验策略时，实现 ValidationStrategy trait 并注册到 StrategyRegistry

4. **告警渠道多元化**
   - 当前对接 li::notification
   - 扩展方向：支持 Slack、钉钉、邮件等多通道，通过配置选择

5. **修复历史追溯**
   - 当前仅记录日志
   - 扩展方向：将校验与修复过程持久化，支持历史查询和分析
