extern crate sota_client;
extern crate url;
extern crate env_logger;

use sota_client::rvi;

use std::env;
use std::sync::mpsc::channel;
use std::thread;
use url::Url;

// TODO: Add error handling, remove `unwrap()`
// TODO: Solve this rvi_url mess
/// Start a SOTA client service listenenig on the provided address/port combinations
fn main() {
    env_logger::init().unwrap();

    let mut args = env::args();
    args.next();
    let rvi_string = args.next().unwrap_or(
        "http://localhost:8901".to_string());
    let edge_string = args.next().unwrap_or(
        "http://localhost:18901".to_string());

    let rvi_url = Url::parse(rvi_string.as_ref()).unwrap();
    let edge_url = Url::parse(edge_string.as_ref()).unwrap();

    let rvi_edge = rvi::RviServiceEdge::new(rvi_url.clone(),
                                            edge_url.clone());

    let (tx, rx) = channel();
    let txc = tx.clone();
    let url = Url::parse(rvi_string.as_ref()).unwrap();
    thread::spawn(move || {
        rvi_edge.start(rvi::RviServiceHandler::new(txc, url));
    });

    loop {
        let e = rx.recv().unwrap();
        let rvi_url = Url::parse(rvi_string.as_ref()).unwrap();
        // TODO: ask the user (dbus) instead of just initiating
        rvi::initiate_download(rvi_url, e);
    }
}
