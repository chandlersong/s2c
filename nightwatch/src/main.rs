use crate::clients::{fetch_data, ping_exchange};
use crate::prometheus_server::PrometheusServer;

use crate::utils::setup_logger;
use hyper::{
    header::CONTENT_TYPE,
    service::{make_service_fn, service_fn},
    Body, Request, Response, Server,
};
use log::{error, info, LevelFilter};

mod prometheus_server;
mod settings;
mod clients;
mod models;
mod errors;
mod utils;

async fn serve_req(_req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    let mut server = PrometheusServer::new();
    match fetch_data().await {
        Ok(result) => server.extend_gauges(result),
        Err(error) => println!("Error: {}", error),
    };
    let buffer = server.print_metric();


    let response = Response::builder()
        .status(200)
        .header(CONTENT_TYPE, server.format_type)
        .body(Body::from(buffer))
        .unwrap();

    Ok(response)
}


#[tokio::main]
async fn main() {
    let _ = setup_logger(Some(LevelFilter::Info));

    if let Err(err) = ping_exchange().await {
        error!("connect exchange failed: {}", err);
    }


    let addr = ([127, 0, 0, 1], 9898).into();
    info!("Listening on http://{}", addr);

    let serve_future = Server::bind(&addr).serve(make_service_fn(|_| async {
        Ok::<_, hyper::Error>(service_fn(serve_req))
    }));

    if let Err(err) = serve_future.await {
        error!("server error: {}", err);
    }

}
