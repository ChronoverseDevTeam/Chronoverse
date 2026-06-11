use axum::{
    body::Body,
    http::{HeaderMap, Method},
    response::Response,
};
use reqwest::Client;

/// Forward a request to the target URL and return the response.
pub async fn proxy_request(
    client: &Client,
    target: &str,
    method: &Method,
    headers: &HeaderMap,
    body: &[u8],
    ticket: Option<&str>,
) -> Result<Response, String> {
    let mut req = match method.as_str() {
        "GET" => client.get(target),
        "POST" => client.post(target),
        "PUT" => client.put(target),
        "DELETE" => client.delete(target),
        _ => client.get(target),
    };

    // Inject auth ticket
    if let Some(t) = ticket {
        req = req.header("Authorization", format!("Ticket {t}"));
    }

    // Forward relevant headers
    if let Some(ct) = headers.get("content-type") {
        req = req.header("content-type", ct.to_str().unwrap_or("application/octet-stream"));
    }

    // Add body for POST/PUT
    if (method == Method::POST || method == Method::PUT) && !body.is_empty() {
        req = req.body(body.to_vec());
    }

    let resp = req.send().await.map_err(|e| format!("upstream error: {e}"))?;
    let status = resp.status();
    let resp_headers = resp.headers().clone();
    let resp_body = resp.bytes().await.map_err(|e| format!("read error: {e}"))?;

    let mut response = Response::builder().status(status);
    for (k, v) in resp_headers.iter() {
        if k != "transfer-encoding" && k != "connection" {
            response = response.header(k, v);
        }
    }

    Ok(response
        .body(Body::from(resp_body.to_vec()))
        .map_err(|e| format!("build response: {e}"))?)
}
