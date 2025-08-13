use std::sync::OnceLock;
use ureq::Agent;

pub(crate) static HTTP_CLIENT: OnceLock<Agent> = OnceLock::new();

pub fn init_http_client(proxy: Option<&str>) -> &'static Agent {
    //TODO：像超时这类进行配置。
    HTTP_CLIENT.get_or_init(|| {
        let mut builder = ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(60));
        if let Some(proxy_url) = proxy {
            let proxy_obj = ureq::Proxy::new(proxy_url).unwrap();
            builder = builder.proxy(proxy_obj);
        };
        builder.build()
    })
}
