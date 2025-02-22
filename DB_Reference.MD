# 说明

主要说明内存数据RocksDB的Key值的命名规范。

# 基本概念

- exchange
    - BN：币安
- 时间：unix的时间
    - seconds：unix的秒
    - millSeconds： unix的毫秒
- tradeType:
    - spot
    - swap

# 行情数据

## Ticker数据

exchange:tradeType:symbol:seconds

## 逐笔数据

exchange:tradeType:symbol:id:millSeconds

id:自动生成的序列

## 深度数据

exchange:tradeType:symbol:millSeconds

# 行情数据

## 账户快照

exchange:account:second

## transaction

exchange:account:"trans":second

trans: 为固定字符串
