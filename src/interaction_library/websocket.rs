use rustc_serialize::{json, Decodable, Encodable};
use std::collections::HashMap;
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use ws::util::Token;
use ws::{listen, Sender as WsSender, Handler, Message, Handshake, CloseCode};
use ws;

use super::gateway::Gateway;


type Clients = Arc<Mutex<HashMap<Token, WsSender>>>;

pub struct WebsocketHandler {
    out:     WsSender,
    sender:  Sender<String>,
    clients: Clients
}

impl Handler for WebsocketHandler {

    fn on_message(&mut self, msg: Message) -> ws::Result<()> {
        Ok(match self.sender.send(format!("{}", msg)) {
            Ok(_) => {},
            Err(e) => error!("Error forwarding message from WS: {}", e)
        })
    }

    fn on_open(&mut self, _: Handshake) -> ws::Result<()> {
        let mut map = self.clients.lock().expect("Poisoned map lock -- can't continue");
        let _ = map.insert(self.out.token(), self.out.clone());
        Ok(())

    }

    fn on_close(&mut self, _: CloseCode, _: &str) {
        let mut map = self.clients.lock().expect("Poisoned map lock -- can't continue");
        let _ = map.remove(&self.out.token().clone());
    }
}

pub struct Websocket {
    clients:  Clients,
    receiver: Mutex<Receiver<String>>,
}

impl<C, E> Gateway<C, E> for Websocket
    where
    C: Decodable + Send + 'static,
    E: Encodable + Send + 'static {

    fn new() -> Websocket {

        fn spawn(tx: Sender<String>, clients: Clients) {

            thread::spawn(move || {
                listen("127.0.0.1:3012", |out| {
                    WebsocketHandler {
                        out:     out,
                        sender:  tx.clone(),
                        clients: clients.clone(),
                    }
                })
            });

        }

        let (tx, rx) = mpsc::channel();
        let clients  = Arc::new(Mutex::new(HashMap::new()));

        spawn(tx, clients.clone());

        Websocket {
            clients:  clients.clone(),
            receiver: Mutex::new(rx),
        }
    }

    fn get_line(&self) -> String {
        let rx = self.receiver.lock().expect("Poisoned rx lock -- can't continue");
        match rx.recv() {
            Ok(line) => line,
            Err(e) => {
                error!("Couldn't fetch from WS receiver: {:?}", e);
                "".to_string()
            }
        }
    }

    fn put_line(&self, s: String) {
        let map = self.clients.lock().expect("Poisoned map lock -- can't continue");
        for (_, out) in map.iter() {
            let _ = out.send(Message::Text(s.clone()));
        }
    }

    fn parse(s: String) -> Option<C> {
        json::decode(&s).ok()
    }

    fn pretty_print(e: E) -> String {
        json::encode(&e).expect("Error encoding event into JSON")
    }

}
