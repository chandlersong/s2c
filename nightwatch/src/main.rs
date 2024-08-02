mod prometheus_server;

use hyper::{
    header::CONTENT_TYPE,
    service::{make_service_fn, service_fn},
    Body, Request, Response, Server,
};
use crate::prometheus_server::PrometheusServer;

async fn serve_req(_req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    let mut server = PrometheusServer::new("stg");
    server.add_new_symbol("coinA", "field", 1.1f64, "open");
    server.add_new_symbol("coinB", "field", 2.1f64, "close");


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
    let addr = ([127, 0, 0, 1], 9898).into();
    println!("Listening on http://{}", addr);

    let serve_future = Server::bind(&addr).serve(make_service_fn(|_| async {
        Ok::<_, hyper::Error>(service_fn(serve_req))
    }));

    if let Err(err) = serve_future.await {
        eprintln!("server error: {}", err);
    }
}
