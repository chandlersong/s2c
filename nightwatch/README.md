# 简介

监控你的账户。作为一个prometheus的一个客户端来给提供数据。需要结合prometheus生态使用。

## 限制

1. 现在支持币安的统一账户合约版本

# 使用

建议通过docker来使用。镜像为chandlersong/nightwatch。下面是一个简单的docker compose的配置

```yaml
nightwatch:
  image: chandlersong/nightwatch:latest
  restart: always
  volumes:
    - ./conf/nightwatch-settings.toml:/app/Settings.toml
  environment:
    - NIGHT_WATCH_CONFIG=/app/Settings.toml
```

1. 环境变量为必须。指向容器内的地址
2. 配置文件参考[Settings.toml](tests/Settings.toml)



