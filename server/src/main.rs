use actix_cors::Cors;
use actix_multipart::Multipart;
use futures_util::stream::StreamExt as _;
use actix_web::{get, post, App, HttpResponse, HttpServer, Responder};
use dotenv::dotenv;
use std::env;
use std::fs;
use std::thread;
use std::io::Write;
use uuid::Uuid;
use std::ffi::OsStr;

mod precrypt_key;
use crate::precrypt_key::*;

mod precrypt_file;

const THREADS: usize = 10;
const MEM_SIZE: usize = 50000000;

#[post("/file/store")]
async fn store_file(mut payload: Multipart) -> impl Responder {
    let file_uuid: String = Uuid::new_v4().to_simple().to_string();
    fs::create_dir(&file_uuid).unwrap();

    let raw_file_string = format!("{}/plaintext.bin", file_uuid);
    let raw_file_path = OsStr::new(&raw_file_string);

    // Write file to disk using multipart stream
    let mut file_count = 0;
    while let Some(item) = payload.next().await {
        file_count += 1;
        if file_count > 1 {
            panic!("Only submit one file at a time.");
        }
        let mut field = item.unwrap();
        println!(
            "Uploading: {}",
            field.content_disposition().unwrap().get_filename().unwrap()
        );

        let mut out = fs::OpenOptions::new()
            .write(true)
            .append(true)
            .create_new(true)
            .open(raw_file_path)
            .unwrap();

        while let Some(chunk) = field.next().await {
            out.write(&chunk.unwrap()).unwrap();
        }
    }

    let file_uuid_c = file_uuid.clone();
    thread::spawn(move || {
        precrypt_file::store_file::store(file_uuid_c, THREADS, MEM_SIZE);
    });
    return HttpResponse::Ok().body(file_uuid);
}

// Generates Orion keys to be used for IPFS storage
#[get("/keygen")]
async fn keygen() -> impl Responder {
    let secret_key = orion::aead::SecretKey::default();
    let secret_key_str = serde_json::to_string(&secret_key.unprotected_as_bytes()).unwrap();
    return HttpResponse::Ok().body(&secret_key_str);
}

#[post("/key/store")]
async fn key_store(req_body: String) -> impl Responder {
    let request: store_key::KeyStoreRequest = serde_json::from_str(&req_body).unwrap();
    let orion_string = env::var("ORION_SECRET").unwrap();
    let web3_token = env::var("WEB3").unwrap();
    let response = store_key::store(request, orion_string, web3_token)
        .await
        .unwrap();
    return HttpResponse::Ok().body(response);
}

#[post("/key/request")]
async fn key_request(req_body: String) -> impl Responder {
    let request: request_key::RecryptRequest = serde_json::from_str(&req_body).unwrap();
    let secret_string = env::var("ORION_SECRET").unwrap();
    let response = request_key::request(request, secret_string).await.unwrap();
    return HttpResponse::Ok().body(response);
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    let host = match env::var("SERVER_HOST") {
        Ok(host) => host,
        Err(_e) => "0.0.0.0:8000".to_string(),
    };

    println!("Starting server on {:?}", host);
    HttpServer::new(|| {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_header()
            .allowed_methods(vec!["POST"])
            .max_age(3600);

        App::new()
            .wrap(cors)
            .service(key_store)
            .service(key_request)
            .service(keygen)
            .service(store_file)
    })
    .bind(host)?
    .run()
    .await
}
