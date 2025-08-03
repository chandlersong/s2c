use reqwest::Client;
use std::sync::OnceLock;
pub(crate) static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

pub fn init_http_client(proxy: Option<&str>) -> &'static reqwest::Client {
    let builder = reqwest::Client::builder();
    let proxy_builder = match proxy {
        Some(val) => { builder.proxy(reqwest::Proxy::https(val).unwrap()) }
        None => { builder }
    };
    HTTP_CLIENT.get_or_init(|| proxy_builder.build().unwrap())
}
