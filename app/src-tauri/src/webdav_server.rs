use actix_web::{web, HttpRequest, HttpResponse, Responder};
use crate::TelegramState;
use std::sync::Arc;

pub async fn handle_propfind(
    path: web::Path<String>,
    _tg_state: web::Data<Arc<TelegramState>>,
) -> impl Responder {
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

    HttpResponse::Ok()
        .insert_header(("Content-Type", "application/xml"))
        .status(actix_web::http::StatusCode::MULTI_STATUS)
        .body(body)
}

pub async fn handle_get(
    path: web::Path<String>,
) -> impl Responder {
    log::info!("[WebDAV] GET: {}", path.into_inner());
    HttpResponse::NotFound().finish()
}

pub async fn handle_put(
    path: web::Path<String>,
    _payload: web::Bytes,
) -> impl Responder {
    log::info!("[WebDAV] PUT: {}", path.into_inner());
    HttpResponse::Created().finish()
}

pub async fn handle_delete(
    path: web::Path<String>,
) -> impl Responder {
    log::info!("[WebDAV] DELETE: {}", path.into_inner());
    HttpResponse::NoContent().finish()
}

pub async fn handle_mkcol(
    path: web::Path<String>,
) -> impl Responder {
    log::info!("[WebDAV] MKCOL: {}", path.into_inner());
    HttpResponse::Created().finish()
}

pub async fn handle_options() -> impl Responder {
    HttpResponse::Ok()
        .insert_header(("Allow", "OPTIONS,GET,PUT,DELETE,MKCOL,PROPFIND"))
        .insert_header(("DAV", "1"))
        .finish()
}

pub async fn handle_catch_all(
    method: actix_web::http::Method,
    path: web::Path<String>,
) -> impl Responder {
    let path_str = path.into_inner();
    log::info!("[WebDAV] {} {}", method, path_str);
    
    match method {
        actix_web::http::Method::GET => HttpResponse::NotFound().finish(),
        actix_web::http::Method::PUT => HttpResponse::Created().finish(),
        actix_web::http::Method::DELETE => HttpResponse::NoContent().finish(),
        _ => HttpResponse::MethodNotAllowed().finish(),
    }
}

pub fn configure_webdav(cfg: &mut web::ServiceConfig) {
    cfg
        .route("/webdav/", web::method(actix_web::http::Method::OPTIONS).to(handle_options))
        .route("/webdav/{path:.*}", web::method(actix_web::http::Method::OPTIONS).to(handle_options))
        .route("/webdav/", web::get().to(handle_get))
        .route("/webdav/{path:.*}", web::get().to(handle_get))
        .route("/webdav/", web::put().to(handle_put))
        .route("/webdav/{path:.*}", web::put().to(handle_put))
        .route("/webdav/", web::delete().to(handle_delete))
        .route("/webdav/{path:.*}", web::delete().to(handle_delete))
        .default_service(web::route().to(handle_catch_all));
}
