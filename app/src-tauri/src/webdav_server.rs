use actix_web::{web, HttpRequest, HttpResponse, http};
use crate::{TelegramState, models::{FileMetadata, FolderMetadata}};
use std::sync::Arc;
use grammers_client::types::{Media, Peer};
use grammers_client::InputMessage;
use crate::commands::utils::resolve_peer;
use chrono::Utc;
use tokio::io::AsyncReadExt;

/// Parse WebDAV path to extract folder and file components
async fn parse_webdav_path(
    path: &str,
    tg_state: &Arc<TelegramState>,
) -> Result<(Option<i64>, Vec<String>), String> {
    let path = path.trim_matches('/');
    
    if path.is_empty() {
        return Ok((None, vec![])); // Root (Saved Messages)
    }
    
    let components: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    
    if components.is_empty() {
        return Ok((None, vec![]));
    }
    
    // Get all folders
    let client_opt = { tg_state.client.lock().await.clone() };
    let client = client_opt.ok_or_else(|| "Client not connected".to_string())?;
    
    let mut folders = Vec::new();
    let mut dialogs = client.iter_dialogs();
    
    while let Some(dialog) = dialogs.next().await.map_err(|e| e.to_string())? {
        if let Peer::Channel(ref c) = dialog.peer {
            let name = c.raw.title.clone();
            if name.to_lowercase().contains("[td]") {
                let display_name = name
                    .replace(" [TD]", "")
                    .replace(" [td]", "")
                    .replace("[TD]", "")
                    .replace("[td]", "")
                    .trim()
                    .to_string();
                folders.push((c.raw.id, display_name));
            }
        }
    }
    
    // Try to match first component to a folder
    let first_component = urlencoding::decode(components[0])
        .unwrap_or_else(|_| components[0].into())
        .to_string();
    
    if let Some((folder_id, _)) = folders.iter().find(|(_, name)| name == &first_component) {
        let remaining: Vec<String> = components[1..]
            .iter()
            .map(|s| urlencoding::decode(s).unwrap_or_else(|_| (*s).into()).to_string())
            .collect();
        Ok((Some(*folder_id), remaining))
    } else {
        let remaining: Vec<String> = components
            .iter()
            .map(|s| urlencoding::decode(s).unwrap_or_else(|_| (*s).into()).to_string())
            .collect();
        Ok((None, remaining))
    }
}

/// Get all folders and root files
async fn get_folder_contents(
    folder_id: Option<i64>,
    tg_state: &Arc<TelegramState>,
) -> Result<(Vec<FolderMetadata>, Vec<FileMetadata>), String> {
    let client_opt = { tg_state.client.lock().await.clone() };
    let client = client_opt.ok_or_else(|| "Client not connected".to_string())?;
    
    let mut folders = Vec::new();
    let mut files = Vec::new();
    
    // If querying root, list all folders
    if folder_id.is_none() {
        let mut dialogs = client.iter_dialogs();
        while let Some(dialog) = dialogs.next().await.map_err(|e| e.to_string())? {
            if let Peer::Channel(ref c) = dialog.peer {
                let name = c.raw.title.clone();
                if name.to_lowercase().contains("[td]") {
                    let display_name = name
                        .replace(" [TD]", "")
                        .replace(" [td]", "")
                        .replace("[TD]", "")
                        .replace("[td]", "")
                        .trim()
                        .to_string();
                    
                    folders.push(FolderMetadata {
                        id: c.raw.id,
                        name: display_name,
                        parent_id: None,
                        username: c.raw.username.clone(),
                        is_public: c.raw.username.is_some(),
                    });
                }
            }
        }
    }
    
    // List files in the current folder
    let peer = resolve_peer(&client, folder_id, &tg_state.peer_cache).await?;
    let mut msgs = client.iter_messages(&peer);
    
    while let Some(msg) = msgs.next().await.map_err(|e| e.to_string())? {
        if let Some(doc) = msg.media() {
            let (name, size, mime, ext) = match doc {
                Media::Document(d) => {
                    let n = d.name().to_string();
                    let s = d.size();
                    let m = d.mime_type().map(|s| s.to_string());
                    let e = std::path::Path::new(&n)
                        .extension()
                        .map(|os| os.to_str().unwrap_or("").to_string());
                    (n, s, m, e)
                }
                Media::Photo(_) => ("Photo.jpg".to_string(), 0, Some("image/jpeg".into()), Some("jpg".into())),
                _ => ("Unknown".to_string(), 0, None, None),
            };
            
            files.push(FileMetadata {
                id: msg.id() as i64,
                folder_id,
                name,
                size: size as u64,
                mime_type: mime,
                file_ext: ext,
                created_at: msg.date().to_string(),
                icon_type: "file".into(),
            });
        }
    }
    
    Ok((folders, files))
}

/// Generate WebDAV PROPFIND response
async fn generate_propfind_response(
    path: &str,
    tg_state: &Arc<TelegramState>,
) -> Result<String, String> {
    let (folder_id, path_components) = parse_webdav_path(path, tg_state).await?;
    
    if path_components.len() > 1 {
        return Err("File does not exist".to_string());
    }
    
    let (folders, files) = get_folder_contents(folder_id, tg_state).await?;
    
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    xml.push_str("<D:multistatus xmlns:D=\"DAV:\">\n");
    
    let href = if folder_id.is_none() {
        "/webdav/".to_string()
    } else {
        format!("/webdav/{}/", path_components.first().unwrap_or(&String::new()))
    };
    
    xml.push_str("  <D:response>\n");
    xml.push_str(&format!("    <D:href>{}</D:href>\n", href));
    xml.push_str("    <D:propstat>\n");
    xml.push_str("      <D:prop>\n");
    xml.push_str("        <D:displayname>Telegram Drive</D:displayname>\n");
    xml.push_str("        <D:resourcetype><D:collection/></D:resourcetype>\n");
    xml.push_str("        <D:getcontenttype>httpd/unix-directory</D:getcontenttype>\n");
    xml.push_str(&format!("        <D:getlastmodified>{}</D:getlastmodified>\n", Utc::now().to_rfc2822()));
    xml.push_str("      </D:prop>\n");
    xml.push_str("      <D:status>HTTP/1.1 200 OK</D:status>\n");
    xml.push_str("    </D:propstat>\n");
    xml.push_str("  </D:response>\n");
    
    for folder in folders {
        let folder_href = format!("/webdav/{}/", urlencoding::encode(&folder.name));
        xml.push_str("  <D:response>\n");
        xml.push_str(&format!("    <D:href>{}</D:href>\n", folder_href));
        xml.push_str("    <D:propstat>\n");
        xml.push_str("      <D:prop>\n");
        xml.push_str(&format!("        <D:displayname>{}</D:displayname>\n", &folder.name));
        xml.push_str("        <D:resourcetype><D:collection/></D:resourcetype>\n");
        xml.push_str("        <D:getcontenttype>httpd/unix-directory</D:getcontenttype>\n");
        xml.push_str(&format!("        <D:getlastmodified>{}</D:getlastmodified>\n", Utc::now().to_rfc2822()));
        xml.push_str("      </D:prop>\n");
        xml.push_str("      <D:status>HTTP/1.1 200 OK</D:status>\n");
        xml.push_str("    </D:propstat>\n");
        xml.push_str("  </D:response>\n");
    }
    
    for file in files {
        let file_href = format!("/webdav/{}", urlencoding::encode(&file.name));
        xml.push_str("  <D:response>\n");
        xml.push_str(&format!("    <D:href>{}</D:href>\n", file_href));
        xml.push_str("    <D:propstat>\n");
        xml.push_str("      <D:prop>\n");
        xml.push_str(&format!("        <D:displayname>{}</D:displayname>\n", &file.name));
        xml.push_str("        <D:resourcetype/>\n");
        xml.push_str(&format!("        <D:getcontenttype>{}</D:getcontenttype>\n", file.mime_type.unwrap_or_else(|| "application/octet-stream".into())));
        xml.push_str(&format!("        <D:getcontentlength>{}</D:getcontentlength>\n", file.size));
        xml.push_str(&format!("        <D:getlastmodified>{}</D:getlastmodified>\n", Utc::now().to_rfc2822()));
        xml.push_str(&format!("        <D:getetag>\"{}-{}\"</D:getetag>\n", file.id, file.size));
        xml.push_str("      </D:prop>\n");
        xml.push_str("      <D:status>HTTP/1.1 200 OK</D:status>\n");
        xml.push_str("    </D:propstat>\n");
        xml.push_str("  </D:response>\n");
    }
    
    xml.push_str("</D:multistatus>\n");
    Ok(xml)
}

pub async fn handle_propfind(
    _req: HttpRequest,
    path: web::Path<String>,
    tg_state: web::Data<Arc<TelegramState>>,
) -> HttpResponse {
    let path_str = path.into_inner();
    log::info!("[WebDAV] PROPFIND: {}", path_str);
    
    match generate_propfind_response(&path_str, &tg_state).await {
        Ok(xml) => HttpResponse::build(http::StatusCode::MULTI_STATUS)
            .content_type("application/xml; charset=utf-8")
            .body(xml),
        Err(e) => {
            log::error!("[WebDAV] PROPFIND error: {}", e);
            HttpResponse::NotFound().finish()
        }
    }
}

pub async fn handle_get(
    _req: HttpRequest,
    path: web::Path<String>,
    tg_state: web::Data<Arc<TelegramState>>,
) -> HttpResponse {
    let path_str = path.into_inner();
    log::info!("[WebDAV] GET: {}", path_str);
    
    match parse_webdav_path(&path_str, &tg_state).await {
        Ok((folder_id, path_components)) => {
            if path_components.is_empty() {
                return HttpResponse::Conflict().finish();
            }
            
            let file_name = &path_components[path_components.len() - 1];
            
            match get_folder_contents(folder_id, &tg_state).await {
                Ok((_, files)) => {
                    if let Some(file) = files.iter().find(|f| f.name == file_name) {
                        let message_id = file.id as i32;
                        let file_size = file.size;
                        let mime_type = file.mime_type.clone().unwrap_or_else(|| "application/octet-stream".into());
                        
                        let tg_state_clone = tg_state.into_inner();
                        let folder_id_clone = folder_id;
                        let file_name_clone = file_name.clone();
                        
                        // Spawn async download task
                        let download_future = async move {
                            let client_opt = { tg_state_clone.client.lock().await.clone() };
                            let client = match client_opt {
                                Some(c) => c,
                                None => return Err("Client not connected".to_string()),
                            };
                            
                            let peer = resolve_peer(&client, folder_id_clone, &tg_state_clone.peer_cache).await?;
                            
                            let messages = client.get_messages_by_id(&peer, &[message_id])
                                .await
                                .map_err(|e| e.to_string())?;
                            
                            let msg = messages.into_iter()
                                .flatten()
                                .next()
                                .ok_or_else(|| "Message not found".to_string())?;
                            
                            let media = msg.media()
                                .ok_or_else(|| "No media in message".to_string())?;
                            
                            let mut download_iter = client.iter_download(&media);
                            let mut data = Vec::new();
                            
                            while let Some(chunk) = download_iter.next().await.transpose() {
                                match chunk {
                                    Ok(bytes) => data.extend_from_slice(&bytes),
                                    Err(e) => return Err(e.to_string()),
                                }
                            }
                            
                            Ok::<Vec<u8>, String>(data)
                        };
                        
                        match tokio::runtime::Runtime::new().map(|rt| rt.block_on(download_future)) {
                            Ok(Ok(data)) => {
                                return HttpResponse::Ok()
                                    .insert_header(("Content-Type", mime_type))
                                    .insert_header(("Content-Length", data.len().to_string()))
                                    .insert_header(("Content-Disposition", format!("attachment; filename=\"{}\"", file_name_clone)))
                                    .body(data);
                            }
                            Ok(Err(e)) => {
                                log::error!("[WebDAV] Download error: {}", e);
                                return HttpResponse::InternalServerError().finish();
                            }
                            Err(e) => {
                                log::error!("[WebDAV] Runtime error: {}", e);
                                return HttpResponse::InternalServerError().finish();
                            }
                        }
                    }
                    HttpResponse::NotFound().finish()
                }
                Err(e) => {
                    log::error!("[WebDAV] GET error: {}", e);
                    HttpResponse::InternalServerError().finish()
                }
            }
        }
        Err(e) => {
            log::error!("[WebDAV] GET path parse error: {}", e);
            HttpResponse::NotFound().finish()
        }
    }
}

pub async fn handle_put(
    _req: HttpRequest,
    path: web::Path<String>,
    payload: web::Bytes,
    tg_state: web::Data<Arc<TelegramState>>,
) -> HttpResponse {
    let path_str = path.into_inner();
    log::info!("[WebDAV] PUT: {} ({} bytes)", path_str, payload.len());
    
    match parse_webdav_path(&path_str, &tg_state).await {
        Ok((folder_id, path_components)) => {
            if path_components.is_empty() {
                return HttpResponse::BadRequest().finish();
            }
            
            let file_name = &path_components[path_components.len() - 1];
            
            // Save to temp file
            let temp_dir = std::env::temp_dir();
            let temp_path = temp_dir.join(format!("webdav_{}", uuid::Uuid::new_v4()));
            
            match tokio::fs::write(&temp_path, &payload).await {
                Ok(_) => {
                    let temp_path_str = temp_path.to_string_lossy().to_string();
                    let file_name_clone = file_name.clone();
                    let tg_state_clone = tg_state.into_inner();
                    
                    let upload_future = async move {
                        let client_opt = { tg_state_clone.client.lock().await.clone() };
                        let client = match client_opt {
                            Some(c) => c,
                            None => return Err("Client not connected".to_string()),
                        };
                        
                        // Upload file to Telegram
                        let uploaded_file = client.upload_stream(
                            &mut tokio::fs::File::open(&temp_path_str).await.map_err(|e| e.to_string())?.into(),
                            payload.len(),
                            file_name_clone.clone()
                        ).await.map_err(|e| e.to_string())?;
                        
                        let message = InputMessage::new().text("").file(uploaded_file);
                        let peer = resolve_peer(&client, folder_id, &tg_state_clone.peer_cache).await?;
                        
                        client.send_message(&peer, message).await.map_err(|e| e.to_string())?;
                        
                        // Cleanup temp file
                        let _ = tokio::fs::remove_file(&temp_path_str).await;
                        
                        Ok::<(), String>(())
                    };
                    
                    match tokio::runtime::Runtime::new().map(|rt| rt.block_on(upload_future)) {
                        Ok(Ok(_)) => {
                            log::info!("[WebDAV] File uploaded successfully: {}", file_name);
                            HttpResponse::Created()
                                .insert_header(("Location", format!("/webdav/{}", urlencoding::encode(file_name))))
                                .finish()
                        }
                        Ok(Err(e)) => {
                            log::error!("[WebDAV] Upload error: {}", e);
                            let _ = std::fs::remove_file(&temp_path);
                            HttpResponse::InternalServerError().finish()
                        }
                        Err(e) => {
                            log::error!("[WebDAV] Runtime error: {}", e);
                            let _ = std::fs::remove_file(&temp_path);
                            HttpResponse::InternalServerError().finish()
                        }
                    }
                }
                Err(e) => {
                    log::error!("[WebDAV] PUT write error: {}", e);
                    HttpResponse::InternalServerError().finish()
                }
            }
        }
        Err(e) => {
            log::error!("[WebDAV] PUT path parse error: {}", e);
            HttpResponse::BadRequest().finish()
        }
    }
}

pub async fn handle_delete(
    _req: HttpRequest,
    path: web::Path<String>,
    tg_state: web::Data<Arc<TelegramState>>,
) -> HttpResponse {
    let path_str = path.into_inner();
    log::info!("[WebDAV] DELETE: {}", path_str);
    
    match parse_webdav_path(&path_str, &tg_state).await {
        Ok((folder_id, path_components)) => {
            if path_components.is_empty() {
                return HttpResponse::Forbidden().finish();
            }
            
            let file_name = &path_components[path_components.len() - 1];
            
            match get_folder_contents(folder_id, &tg_state).await {
                Ok((_, files)) => {
                    if let Some(file) = files.iter().find(|f| f.name == file_name) {
                        let message_id = file.id as i32;
                        let tg_state_clone = tg_state.into_inner();
                        
                        let delete_future = async move {
                            let client_opt = { tg_state_clone.client.lock().await.clone() };
                            let client = match client_opt {
                                Some(c) => c,
                                None => return Err("Client not connected".to_string()),
                            };
                            
                            let peer = resolve_peer(&client, folder_id, &tg_state_clone.peer_cache).await?;
                            client.delete_messages(&peer, &[message_id]).await.map_err(|e| e.to_string())?;
                            
                            Ok::<(), String>(())
                        };
                        
                        match tokio::runtime::Runtime::new().map(|rt| rt.block_on(delete_future)) {
                            Ok(Ok(_)) => {
                                log::info!("[WebDAV] File deleted: {}", file_name);
                                HttpResponse::NoContent().finish()
                            }
                            Ok(Err(e)) => {
                                log::error!("[WebDAV] Delete error: {}", e);
                                HttpResponse::InternalServerError().finish()
                            }
                            Err(e) => {
                                log::error!("[WebDAV] Runtime error: {}", e);
                                HttpResponse::InternalServerError().finish()
                            }
                        }
                    } else {
                        HttpResponse::NotFound().finish()
                    }
                }
                Err(e) => {
                    log::error!("[WebDAV] DELETE error: {}", e);
                    HttpResponse::InternalServerError().finish()
                }
            }
        }
        Err(e) => {
            log::error!("[WebDAV] DELETE path parse error: {}", e);
            HttpResponse::NotFound().finish()
        }
    }
}

pub async fn handle_mkcol(
    _req: HttpRequest,
    path: web::Path<String>,
    tg_state: web::Data<Arc<TelegramState>>,
) -> HttpResponse {
    let path_str = path.into_inner();
    log::info!("[WebDAV] MKCOL: {}", path_str);
    
    let path_str = path_str.trim_matches('/');
    
    let folder_name = urlencoding::decode(path_str)
        .unwrap_or_else(|_| path_str.into())
        .to_string();
    
    if folder_name.is_empty() {
        return HttpResponse::BadRequest().finish();
    }
    
    let tg_state_clone = tg_state.into_inner();
    
    let create_future = async move {
        let client_opt = { tg_state_clone.client.lock().await.clone() };
        let client = match client_opt {
            Some(c) => c,
            None => return Err("Client not connected".to_string()),
        };
        
        use grammers_tl_types as tl;
        
        let result = client.invoke(&tl::functions::channels::CreateChannel {
            broadcast: true,
            megagroup: false,
            title: format!("{} [TD]", folder_name),
            about: "Telegram Drive Storage Folder\n[telegram-drive-folder]".to_string(),
            geo_point: None,
            address: None,
            for_import: false,
            forum: false,
            ttl_period: None,
        }).await.map_err(|e| e.to_string())?;
        
        log::info!("[WebDAV] Folder created: {}", folder_name);
        Ok::<(), String>(())
    };
    
    match tokio::runtime::Runtime::new().map(|rt| rt.block_on(create_future)) {
        Ok(Ok(_)) => HttpResponse::Created().finish(),
        Ok(Err(e)) => {
            log::error!("[WebDAV] Create folder error: {}", e);
            HttpResponse::InternalServerError().finish()
        }
        Err(e) => {
            log::error!("[WebDAV] Runtime error: {}", e);
            HttpResponse::InternalServerError().finish()
        }
    }
}

pub async fn handle_options() -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(("Allow", "OPTIONS,GET,HEAD,PUT,DELETE,MKCOL,PROPFIND"))
        .insert_header(("DAV", "1"))
        .insert_header(("Content-Length", "0"))
        .finish()
}

pub fn configure_webdav(cfg: &mut web::ServiceConfig) {
    cfg
        .route("/webdav", web::method(http::Method::OPTIONS).to(handle_options))
        .route("/webdav/", web::method(http::Method::OPTIONS).to(handle_options))
        .route("/webdav", web::method(http::Method::PROPFIND).to(handle_propfind))
        .route("/webdav/", web::method(http::Method::PROPFIND).to(handle_propfind))
        .route("/webdav/{path:.*}", web::method(http::Method::PROPFIND).to(handle_propfind))
        .route("/webdav/{path:.*}", web::method(http::Method::GET).to(handle_get))
        .route("/webdav/{path:.*}", web::method(http::Method::PUT).to(handle_put))
        .route("/webdav/{path:.*}", web::method(http::Method::DELETE).to(handle_delete))
        .route("/webdav/{path:.*}", web::method(http::Method::MKCOL).to(handle_mkcol))
        .route("/webdav/{path:.*}", web::method(http::Method::OPTIONS).to(handle_options));
}
