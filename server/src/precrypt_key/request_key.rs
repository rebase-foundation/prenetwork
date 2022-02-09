use serde::{Deserialize, Serialize};
use serde_json::value::Value;
use serde_json::json;
use actix_web::client::Client;
use orion::aead;
use nacl::sign::verify;
use std::str;
use umbral_pre::*;
use generic_array::GenericArray;

use crate::store_key::*;

#[derive(Serialize, Deserialize)]
pub struct RecryptRequest {
   cid: String,
   precrypt_pubkey: Vec<u8>,    // recrypt key
   sol_pubkey: Vec<u8>,         // sol pubkey
   sol_signed_message: Vec<u8>, // sol signed message
}

#[derive(Serialize, Deserialize)]
struct SolanaJSONRPCResult {
   result: SolanaJSONRPCResultValue,
}

#[derive(Serialize, Deserialize)]
struct SolanaJSONRPCResultValue {
   value: Vec<Value>,
}

pub async fn request(
   request: RecryptRequest,
   orion_secret: String
) -> std::io::Result<String> {

   // Get the data from IFPS
   let client = Client::default();
   let response = client
      .get(format!("https://ipfs.io/ipfs/{}", request.cid))
      .send()
      .await;

   let response_body_bytes = response.unwrap().body().await.unwrap();
   let response_body_str: String = serde_json::from_slice(&response_body_bytes).unwrap();
   let response_body: Vec<u8> = serde_json::from_str(&response_body_str).unwrap();
   
   // Decrypt the data with private key
   let secret_slice: Vec<u8> = serde_json::from_str(&orion_secret).unwrap();
   let secret_key = aead::SecretKey::from_slice(&secret_slice).unwrap();
   let decrypted_bytes = aead::open(&secret_key, &response_body).unwrap();
   let decrypted_str = str::from_utf8(&decrypted_bytes).unwrap();
   let data: KeyStoreRequest = serde_json::from_str(&decrypted_str).unwrap();
   let mint = data.mint;
   let recryption_keys = data.recryption_keys;

   // Verify that the getter holds the token
   // Verify signature
   let signed = verify(
      &request.sol_signed_message,
      "precrypt".as_bytes(),
      &request.sol_pubkey,
   )
   .unwrap();
   if !signed {
      panic!("Signature verification failed");
   }

   // Encode pubkey bytes to string
   let sol_pubkey = bs58::encode(request.sol_pubkey).into_string();

   // Verify solana pubkey owns token from mint
   let client = Client::default();
   let response = client
      .post("https://ssc-dao.genesysgo.net/")
      .header("Content-Type", "application/json")
      .send_body(json!({
          "jsonrpc": "2.0",
          "id": 1,
          "method": "getTokenAccountsByOwner",
          "params": [
              sol_pubkey,
              {
                  "mint": mint
              },
              {
                  "encoding": "jsonParsed"
              }
          ]
      }))
      .await;

   let response_body_bytes = response.unwrap().body().await.unwrap();
   let response_body: SolanaJSONRPCResult = serde_json::from_slice(&response_body_bytes).unwrap();
   let values = response_body.result.value;
   let mut owns_token = false;
   for value in values {
      let balance_str = value
         .get("account")
         .unwrap()
         .get("data")
         .unwrap()
         .get("parsed")
         .unwrap()
         .get("info")
         .unwrap()
         .get("tokenAmount")
         .unwrap()
         .get("uiAmountString")
         .unwrap();
      let balance: f64 = balance_str.as_str().unwrap().parse::<f64>().unwrap();
      if balance >= 1.0 {
         owns_token = true;
      }
   }
   if !owns_token {
      panic!("Solana account doesn't own required token");
   }

   // Generate the decryption keys
   let precrypt_pubkey =
      PublicKey::from_array(&GenericArray::from_iter(request.precrypt_pubkey)).unwrap();
   let decryption_keys = precrypt::recrypt(recryption_keys, precrypt_pubkey).unwrap();
   return Ok(serde_json::to_string(&decryption_keys).unwrap());
}