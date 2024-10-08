use crate::clients::{cal_gauge_according_setting, ping_exchange};
use crate::prometheus_server::PrometheusServer;

use braavos::utils::setup_logger;
use hyper::{
    header::CONTENT_TYPE,
    service::{make_service_fn, service_fn},
    Body, Request, Response, Server,
};
use log::{error, info, LevelFilter};

mod prometheus_server;
mod clients;
mod errors;

async fn serve_req(_req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    let mut server = PrometheusServer::new();
    match cal_gauge_according_setting().await {
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


    let addr = ([0, 0, 0, 0], 9898).into();
    info!("Listening on http://{}", addr);

    let serve_future = Server::bind(&addr).serve(make_service_fn(|_| async {
        Ok::<_, hyper::Error>(service_fn(serve_req))
    }));

    if let Err(err) = serve_future.await {
        error!("server error: {}", err);
    }

}
