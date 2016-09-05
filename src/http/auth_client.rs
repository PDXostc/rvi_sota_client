use chan::Sender;
use hyper;
use hyper::{Encoder, Decoder, Next};
use hyper::client::{Client as HyperClient, Handler, HttpsConnector,
                    Request as HyperRequest, Response as HyperResponse};
use hyper::header::{Authorization, Basic, Bearer, ContentLength, ContentType, Location};
use hyper::mime::{Attr, Mime, TopLevel, SubLevel, Value};
use hyper::net::{HttpStream, HttpsStream, OpensslStream};
use hyper::status::StatusCode;
use std::{io, mem};
use std::io::{ErrorKind, Write};
use std::str;
use std::time::Duration;
use time;

use datatype::{Auth, Error};
use http::{Client, get_openssl, Request, Response};


/// The `AuthClient` will attach an `Authentication` header to each outgoing
/// HTTP request.
#[derive(Clone)]
pub struct AuthClient {
    auth:   Auth,
    client: HyperClient<AuthHandler>,
}

impl Default for AuthClient {
    fn default() -> Self {
        Self::from(Auth::None)
    }
}

impl AuthClient {
    /// Instantiates a new client ready to make requests for the given `Auth` type.
    pub fn from(auth: Auth) -> Self {
        let client  = HyperClient::<AuthHandler>::configure()
            .keep_alive(true)
            .max_sockets(1024)
            .connector(HttpsConnector::new(get_openssl()))
            .build()
            .expect("unable to create a new hyper Client");

        AuthClient {
            auth:   auth,
            client: client,
        }
    }
}

impl Client for AuthClient {
    fn chan_request(&self, req: Request, resp_tx: Sender<Response>) {
        info!("{} {}", req.method, req.url);
        let _ = self.client.request(req.url.inner(), AuthHandler {
            auth:     self.auth.clone(),
            req:      req,
            timeout:  Duration::from_secs(20),
            started:  None,
            written:  0,
            response: Vec::new(),
            resp_tx:  resp_tx.clone(),
        }).map_err(|err| resp_tx.send(Err(Error::from(err))));
    }
}


/// The async handler for outgoing HTTP requests.
// FIXME: uncomment when yocto is at 1.8.0: #[derive(Debug)]
pub struct AuthHandler {
    auth:     Auth,
    req:      Request,
    timeout:  Duration,
    started:  Option<u64>,
    written:  usize,
    response: Vec<u8>,
    resp_tx:  Sender<Response>,
}

// FIXME: required for building on 1.7.0 only
impl ::std::fmt::Debug for AuthHandler {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        write!(f, "unimplemented")
    }
}

impl AuthHandler {
    fn redirect_request(&mut self, resp: HyperResponse) {
        match resp.headers().get::<Location>() {
            Some(&Location(ref loc)) => self.req.url.join(loc).map(|url| {
                debug!("redirecting to {}", url);
                // drop Authentication Header on redirect
                let client  = AuthClient::default();
                let resp_rx = client.send_request(Request {
                    url:    url,
                    method: self.req.method.clone(),
                    body:   mem::replace(&mut self.req.body, None),
                });
                match resp_rx.recv().expect("no redirect_request response") {
                    Ok(data) => self.resp_tx.send(Ok(data)),
                    Err(err) => self.resp_tx.send(Err(Error::from(err)))
                }
            }).unwrap_or_else(|err| self.resp_tx.send(Err(Error::from(err)))),

            None => self.resp_tx.send(Err(Error::Client("redirect missing Location header".to_string())))
        }
    }
}

/// The `AuthClient` may be used for both HTTP and HTTPS connections.
pub type Stream = HttpsStream<OpensslStream<HttpStream>>;

impl Handler<Stream> for AuthHandler {
    fn on_request(&mut self, req: &mut HyperRequest) -> Next {
        req.set_method(self.req.method.clone().into());
        self.started    = Some(time::precise_time_ns());
        let mut headers = req.headers_mut();

        // empty Charset to keep RVI happy
        let mime_json = Mime(TopLevel::Application, SubLevel::Json, vec![]);
        let mime_form = Mime(TopLevel::Application, SubLevel::WwwFormUrlEncoded,
                             vec![(Attr::Charset, Value::Utf8)]);

        match self.auth {
            Auth::None => {
                headers.set(ContentType(mime_json));
            }

            Auth::Credentials(_, _) if self.req.body.is_some() => {
                panic!("no request body expected for Auth::Credentials");
            }

            Auth::Credentials(ref id, ref secret) => {
                headers.set(Authorization(Basic { username: id.0.clone(),
                                                  password: Some(secret.0.clone()) }));
                headers.set(ContentType(mime_form));
                self.req.body = Some(br#"grant_type=client_credentials"#.to_vec());
            }

            Auth::Token(ref token) => {
                headers.set(Authorization(Bearer { token: token.access_token.clone() }));
                headers.set(ContentType(mime_json));
            }
        };

        self.req.body.as_ref().map_or(Next::read().timeout(self.timeout), |body| {
            headers.set(ContentLength(body.len() as u64));
            Next::write()
        })
    }

    fn on_request_writable(&mut self, encoder: &mut Encoder<Stream>) -> Next {
        let body = self.req.body.as_ref().expect("on_request_writable expects a body");

        match encoder.write(&body[self.written..]) {
            Ok(0) => {
                info!("Request length: {} bytes", body.len());
                if let Ok(body) = str::from_utf8(body) {
                    debug!("body:\n{}", body);
                }
                Next::read().timeout(self.timeout)
            },

            Ok(n) => {
                self.written += n;
                trace!("{} bytes written to request body", n);
                Next::write()
            }

            Err(ref err) if err.kind() == ErrorKind::WouldBlock => {
                trace!("retry on_request_writable");
                Next::write()
            }

            Err(err) => {
                error!("unable to write request body: {}", err);
                self.resp_tx.send(Err(Error::from(err)));
                Next::remove()
            }
        }
    }

    fn on_response(&mut self, resp: HyperResponse) -> Next {
        info!("Response status: {}", resp.status());
        debug!("on_response headers:\n{}", resp.headers());
        let started = self.started.expect("expected start time");
        let latency = time::precise_time_ns() as f64 - started as f64;
        debug!("on_response latency: {}ms", (latency / 1e6) as u32);

        if resp.status().is_success() {
            if let Some(len) = resp.headers().get::<ContentLength>() {
                if **len > 0 {
                    return Next::read();
                }
            }
            self.resp_tx.send(Ok(Vec::new()));
            Next::end()
        } else if resp.status().is_redirection() {
            self.redirect_request(resp);
            Next::end()
        } else if resp.status() == &StatusCode::Forbidden {
            self.resp_tx.send(Err(Error::Authorization(format!("{}", resp.status()))));
            Next::end()
        } else {
            self.resp_tx.send(Err(Error::Client(format!("{}", resp.status()))));
            Next::end()
        }
    }

    fn on_response_readable(&mut self, decoder: &mut Decoder<Stream>) -> Next {
        match io::copy(decoder, &mut self.response) {
            Ok(0) => {
                debug!("on_response_readable bytes read: {}", self.response.len());
                self.resp_tx.send(Ok(mem::replace(&mut self.response, Vec::new())));
                Next::end()
            }

            Ok(n) => {
                trace!("{} more response bytes read", n);
                Next::read()
            }

            Err(ref err) if err.kind() == ErrorKind::WouldBlock => {
                trace!("retry on_response_readable");
                Next::read()
            }

            Err(err) => {
                error!("unable to read response body: {}", err);
                self.resp_tx.send(Err(Error::from(err)));
                Next::end()
            }
        }
    }

    fn on_error(&mut self, err: hyper::Error) -> Next {
        error!("on_error: {}", err);
        self.resp_tx.send(Err(Error::from(err)));
        Next::remove()
    }
}


#[cfg(test)]
mod tests {
    use rustc_serialize::json::Json;
    use std::path::Path;

    use super::*;
    use http::{Client, set_ca_certificates};


    fn get_client() -> AuthClient {
        set_ca_certificates(&Path::new("run/sota_certificates"));
        AuthClient::default()
    }

    #[test]
    fn test_send_get_request() {
        let client  = get_client();
        let url     = "http://eu.httpbin.org/bytes/16?seed=123".parse().unwrap();
        let resp_rx = client.get(url, None);
        let data    = resp_rx.recv().unwrap().unwrap();
        assert_eq!(data, vec![13, 22, 104, 27, 230, 9, 137, 85, 218, 40, 86, 85, 62, 0, 111, 22]);
    }

    #[test]
    fn test_send_post_request() {
        let client  = get_client();
        let url     = "https://eu.httpbin.org/post".parse().unwrap();
        let resp_rx = client.post(url, Some(br#"foo"#.to_vec()));
        let body    = resp_rx.recv().unwrap().unwrap();
        let resp    = String::from_utf8(body).unwrap();
        let json    = Json::from_str(&resp).unwrap();
        let obj     = json.as_object().unwrap();
        let data    = obj.get("data").unwrap().as_string().unwrap();
        assert_eq!(data, "foo");
    }
}
