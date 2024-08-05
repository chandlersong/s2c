#[cfg(test)]
mod tests {

    #[tokio::main]
    #[ignore]
    #[test]
    async fn run_binance_ping() -> Result<(), reqwest::Error> {
        let url = "https://api.binance.com/api/v3/ping";

        eprintln!("Fetching {url:?}...");

        // reqwest::get() is a convenience function.
        //
        // In most cases, you should create/build a reqwest::Client and reuse
        // it for all requests.
        let http_proxy = reqwest::Proxy::http("http://localhost:7890")?;
        // let https_proxy = reqwest::Proxy::https("http://localhost:7890")?;

        let client = reqwest::Client::builder()
            // .proxy(https_proxy)
            .proxy(http_proxy)
            .build().unwrap();

        let res = client.get(url).send().await?;

        eprintln!("Response: {:?} {}", res.version(), res.status());
        eprintln!("Headers: {:#?}\n", res.headers());

        let body = res.text().await?;

        println!("{body}");

        Ok(())
    }
}
