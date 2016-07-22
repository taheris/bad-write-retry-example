extern crate hyper;
extern crate testopenssl;
extern crate url;

use hyper::Method;

use testopenssl::{HttpRequest, TestClient};


fn main() {
    let client  = TestClient::new();
    let resp_rx = client.request(HttpRequest {
        method: Method::Post,
        url:    url::Url::parse("https://eu.httpbin.org/post").unwrap(),
        body:   vec![b'X'; 1000000],
    });
    let _ = resp_rx.recv().unwrap().unwrap();
}
