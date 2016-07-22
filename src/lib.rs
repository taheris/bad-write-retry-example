extern crate hyper;
extern crate rustc_serialize;
extern crate url;

use hyper::{Encoder, Decoder, Method, Next};
use hyper::client::{Client, Handler, HttpsConnector, Request, Response};
use hyper::header::{ContentLength, ContentType};
use hyper::mime::{Attr, Mime, TopLevel, SubLevel, Value};
use hyper::net::{HttpStream, HttpsStream, OpensslStream, Openssl};
use std::{io, mem};
use std::io::{ErrorKind, Write};
use std::sync::mpsc::{channel, Sender, Receiver};
use std::time::Duration;


macro_rules! output(
    ($($arg:tt)*) => { {
        let _ = writeln!(&mut io::stdout(), $($arg)*);
    } }
);

pub struct HttpRequest {
    pub method: Method,
    pub url:    url::Url,
    pub body:   Vec<u8>
}

pub type HttpResponse = Result<Vec<u8>, String>;


pub struct TestClient {
    client: Client<TestHandler>,
}

impl TestClient {
    pub fn new() -> TestClient {
        TestClient {
            client: Client::<TestHandler>::configure()
                .keep_alive(true)
                .max_sockets(1024)
                .connector(HttpsConnector::new(Openssl::default()))
                .build()
                .expect("unable to create a new hyper Client")
        }
    }

    pub fn request(&self, req: HttpRequest) -> Receiver<HttpResponse>{
        output!("send_request_to: {:?}", req.url);
        let (resp_tx, resp_rx) = channel();
        let _ = self.client.request(req.url.clone(), TestHandler {
            req:      req,
            written:  0,
            response: Vec::new(),
            resp_tx:  resp_tx.clone(),
        }).map_err(|err| resp_tx.send(Err(err.to_string())));
        resp_rx
    }
}


pub struct TestHandler {
    req:      HttpRequest,
    written:  usize,
    response: Vec<u8>,
    resp_tx:  Sender<HttpResponse>,
}


pub type Stream = HttpsStream<OpensslStream<HttpStream>>;

impl Handler<Stream> for TestHandler {
    fn on_request(&mut self, req: &mut Request) -> Next {
        req.set_method(self.req.method.clone().into());
        output!("on_request: {} {}", req.method(), req.uri());
        let mut headers = req.headers_mut();
        headers.set(ContentType(Mime(TopLevel::Application, SubLevel::Json, vec![(Attr::Charset, Value::Utf8)])));
        headers.set(ContentLength(self.req.body.len() as u64));
        Next::write()
    }

    fn on_request_writable(&mut self, encoder: &mut Encoder<Stream>) -> Next {
        match encoder.write(&self.req.body[self.written..]) {
            Ok(0) => {
                output!("{} bytes written to request body", self.written);
                Next::read().timeout(Duration::from_secs(10))
            },

            Ok(n) => {
                self.written += n;
                output!("{} bytes written to request body", n);
                Next::write()
            }

            Err(ref err) if err.kind() == ErrorKind::WouldBlock => {
                //output!("retry on_request_writable");
                Next::write()
            }

            Err(err) => {
                output!("unable to write request body: {}", err);
                let _ = self.resp_tx.send(Err(err.to_string()));
                Next::remove()
            }
        }
    }

    fn on_response(&mut self, resp: Response) -> Next {
        output!("on_response status: {}", resp.status());
        output!("on_response headers:\n{}", resp.headers());
        if resp.status().is_success() {
            if let Some(len) = resp.headers().get::<ContentLength>() {
                if **len > 0 {
                    return Next::read();
                }
            }
            let _ = self.resp_tx.send(Ok(Vec::new()));
            Next::end()
        } else {
            let _ = self.resp_tx.send(Err(format!("failed response status: {}", resp.status())));
            Next::end()
        }
    }

    fn on_response_readable(&mut self, decoder: &mut Decoder<Stream>) -> Next {
        match io::copy(decoder, &mut self.response) {
            Ok(0) => {
                output!("on_response_readable bytes read: {:?}", self.response.len());
                let _ = self.resp_tx.send(Ok(mem::replace(&mut self.response, Vec::new())));
                Next::end()
            }

            Ok(n) => {
                output!("{} more response bytes read", n);
                Next::read()
            }

            Err(ref err) if err.kind() == ErrorKind::WouldBlock => {
                //output!("retry on_response_readable");
                Next::read()
            }

            Err(err) => {
                let _ = self.resp_tx.send(Err(format!("unable to read response body: {}", err)));
                Next::end()
            }
        }
    }

    fn on_error(&mut self, err: hyper::Error) -> Next {
        let _ = self.resp_tx.send(Err(format!("on_error: {}", err)));
        Next::remove()
    }
}


#[cfg(test)]
mod tests {
    use hyper::Method;
    use rustc_serialize::json::Json;
    use url;

    use super::*;


    #[test]
    fn test_send_post_request() {
        let client  = TestClient::new();
        let resp_rx = client.request(HttpRequest {
            method: Method::Post,
            url:    url::Url::parse("https://eu.httpbin.org/post").unwrap(),
            body:   br#"foo"#.to_vec(),
        });
        let body = resp_rx.recv().unwrap().unwrap();
        let resp = String::from_utf8(body).unwrap();
        let json = Json::from_str(&resp).unwrap();
        let obj  = json.as_object().unwrap();
        let data = obj.get("data").unwrap().as_string().unwrap();
        assert_eq!(data, "foo");
    }
}
