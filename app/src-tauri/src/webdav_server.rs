use actix_web::{web, HttpRequest, HttpResponse, http};
use crate::TelegramState;
use std::sync::Arc;

pub async fn handle_propfind(
    req: HttpRequest,
    path: web::Path<String>,
    _tg_state: web::Data<Arc<TelegramState>>,
) -> HttpResponse {
    log::info!("[WebDAV] PROPFIND: {}", path.into_inner());
    
    let body = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
    <D:response>
        <D:href>/</D:href>
        <D:propstat>
            <D:prop>
                <D:displayname>Telegram Drive</D:displayname>
                <D:resourcetype><D:collection/></D:resourcetype>
            </D:prop>
            <D:status>HTTP/1.1 200 OK</D:status>
        </D:propstat>
    </D:response>
</D:multistatus>"#;

    HttpResponse::MultiStatus()
        .content_type("application/xml")
        .body(body)
}

pub async fn handle_get(
    _req: HttpRequest,
    path: web::Path<String>,
) -> HttpResponse {
    log::info!("[WebDAV] GET: {}", path.into_inner());
    HttpResponse::NotFound().finish()
}

pub async fn handle_put(
    _req: HttpRequest,
    path: web::Path<String>,
    _payload: web::Bytes,
) -> HttpResponse {
    log::info!("[WebDAV] PUT: {}", path.into_inner());
    HttpResponse::Created().finish()
}

pub async fn handle_delete(
    _req: HttpRequest,
    path: web::Path<String>,
) -> HttpResponse {
    log::info!("[WebDAV] DELETE: {}", path.into_inner());
    HttpResponse::NoContent().finish()
}

pub async fn handle_mkcol(
    _req: HttpRequest,
    path: web::Path<String>,
) -> HttpResponse {
    log::info!("[WebDAV] MKCOL: {}", path.into_inner());
    HttpResponse::Created().finish()
}

pub async fn handle_options() -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(("Allow", "OPTIONS,GET,PUT,DELETE,MKCOL,PROPFIND"))
        .insert_header(("DAV", "1"))
        .finish()
}

pub fn configure_webdav(cfg: &mut web::ServiceConfig) {
    cfg
        .route("/webdav/{path:.*}", web::method(http::Method::PROPFIND).to(handle_propfind))
        .route("/webdav/{path:.*}", web::method(http::Method::GET).to(handle_get))
        .route("/webdav/{path:.*}", web::method(http::Method::PUT).to(handle_put))
        .route("/webdav/{path:.*}", web::method(http::Method::DELETE).to(handle_delete))
        .route("/webdav/{path:.*}", web::method(http::Method::MKCOL).to(handle_mkcol))
        .route("/webdav/", web::method(http::Method::OPTIONS).to(handle_options));
}