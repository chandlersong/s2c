# 数据完整性校验与自动补拉——任务文档

## 新建和改动的文件及目录

### 新建文件
- yu/src/data_integrity/mod.rs
- yu/src/data_integrity/supervisor.rs
- yu/src/data_integrity/checker.rs
- yu/src/data_integrity/repair.rs
- yu/src/data_integrity/models.rs
- yu/src/data_integrity/config.rs
- yu/src/data_integrity/strategy.rs

### 改动文件
- yu/src/lib.rs：加入 data_integrity 模块声明与暴露
- yu/src/config.rs：增加 DataIntegrityConfig 配置加载
- 应用启动入口（yu/src/bin/yu_datacenter.rs 或主 service 文件）：初始化 DataIntegritySupervisor 并启动

## 主要任务和里程碑

### 任务1：模块骨架与核心数据结构
- [x] 任务1.1：创建 data_integrity 模块目录与基础 mod.rs
- [x] 任务1.2：定义 HealthState 枚举（OK / DEGRADED / RECOVERING / FAILED）及相关结构体
- [x] 任务1.3：定义 ValidationResult 事件结构（缺口摘要、重试计数、错误信息）
- [x] 任务1.4：定义 RepairRequest 和 RepairResult 消息结构
- [x] 任务1.5：定义 ValidationStrategy trait 接口与注册表

### 任务2：配置与初始化
- [ ] 任务2.1：在 yu/src/data_integrity/config.rs 定义 DataIntegrityConfig 结构
  - periodic_check_interval_ms
  - periodic_check_window
  - repair_max_concurrency
  - repair_backoff_strategy
  - alert_on_failure_enabled
- [ ] 任务2.2：在 yu/src/config.rs 加入配置加载，支持 YU_* 环境变量覆盖
- [ ] 任务2.3：编写配置默认值设置与验证逻辑

### 任务3：Supervisor 组件
- [ ] 任务3.1：定义 DataIntegritySupervisor Actor 结构
- [ ] 任务3.2：实现 Supervisor 启动时的初始化逻辑（加载配置、初始化 HealthState）
- [ ] 任务3.3：实现 Supervisor 拉起 DataIntegrityChecker Actor
- [ ] 任务3.4：实现 HealthState 状态管理与更新逻辑
- [ ] 任务3.5：实现 GetHealthState Handler（只读查询接口）
- [ ] 任务3.6：实现 Supervisor 接收修复结果消息，更新状态

### 任务4：Checker 组件
- [ ] 任务4.1：定义 DataIntegrityChecker Actor 结构
- [ ] 任务4.2：实现 Checker 启动时的首轮校验逻辑
- [ ] 任务4.3：实现 interval 定时循环校验逻辑
- [ ] 任务4.4：实现调用 ValidationStrategy 的校验流程
- [ ] 任务4.5：实现校验超时控制与异常捕获
- [ ] 任务4.6：实现产出 ValidationResult 事件投递给 RepairExecutor

### 任务5：RepairExecutor 组件
- [ ] 任务5.1：定义 RepairExecutor Actor 结构与并发控制机制
- [ ] 任务5.2：实现接收 ValidationResult 消息的 Handler
- [ ] 任务5.3：实现限流队列与指数退避逻辑
- [ ] 任务5.4：实现补��调用（调用 yue 现有拉取接口，禁止直接 HTTP）
- [ ] 任务5.5：实现补拉后的轻量修复验证（调用 ValidationStrategy）
- [ ] 任务5.6：实现修复结果上报给 Supervisor

### 任务6：整合与启动流程
- [ ] 任务6.1：在 yu/src/lib.rs 暴露 data_integrity 模块与核心接口
- [ ] 任务6.2：在应用启动入口初始化 DataIntegritySupervisor
- [ ] 任务6.3：编写启动逻辑：加载配置 -> 初始化 Supervisor -> 启动 Checker

### 任务7：观测与告警
- [ ] 任务7.1：接入日志打点（校验开始/结束/失败、补拉开始/结束/失败、状态变更）
- [ ] 任务7.2：定义指标收集接口（check_success_total、check_failure_total、check_duration_ms、repair_attempts、repair_failures）
- [ ] 任务7.3：实现告警触发逻辑（对接 li::notification，支持配置开关）
- [ ] 任务7.4：编写日志与指标收集的集成测试

### 任务8：文档与测试
- [ ] 任��8.1：编写 API 文档与使用示例
- [ ] 任务8.2：编写单元测试：Supervisor、Checker、RepairExecutor 各组件的基本功能
- [ ] 任务8.3：编写集成测试：启动 -> 校验 -> 修复的完整流程
- [ ] 任务8.4：验证配置加载与环境变量覆盖
