#[tokio::main]
async fn main() {
    let url = "https://api.binance.com/api/v3/ping";

    eprintln!("Fetching {url:?}...");

    // reqwest::get() is a convenience function.
    //
    // In most cases, you should create/build a reqwest::Client and reuse
    // it for all requests.
    // let http_proxy = reqwest::Proxy::http("http://localhost:7890")?;
    let https_proxy = reqwest::Proxy::https("http://localhost:7890").unwrap();

    let client = reqwest::Client::builder()
        .proxy(https_proxy)
        .build().unwrap();

    let res = client.get(url).send().await.unwrap();

    eprintln!("Response: {:?} {}", res.version(), res.status());
    eprintln!("Headers: {:#?}\n", res.headers());

    let body = res.text().await.unwrap();

    println!("{body}");
}

