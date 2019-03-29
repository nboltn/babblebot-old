extern crate jsonwebtoken as jwt;

use crate::types::*;
use crate::util::*;
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use bcrypt::{DEFAULT_COST, hash, verify};
use config;
use reqwest;
use reqwest::header;
use reqwest::header::HeaderValue;
use rocket::{self, Outcome, routes,get,post,error};
use rocket::http::{Status,Cookie,Cookies};
use rocket::request::{self, Request, FromRequest, Form};
use rocket_contrib::json::Json;
use rocket_contrib::templates::Template;
use rocket_contrib::databases::redis;
use r2d2_redis::redis::Commands;
use jwt::{encode, decode, Header, Algorithm, Validation};

impl<'a, 'r> FromRequest<'a, 'r> for Auth {
    type Error = AuthError;

    fn from_request(request: &'a Request<'r>) -> request::Outcome<Self, Self::Error> {
        let mut settings = config::Config::default();
        settings.merge(config::File::with_name("Settings")).unwrap();
        settings.merge(config::Environment::with_prefix("BABBLEBOT")).unwrap();

        let mut forward = true;
        if let Some(f) = request.headers().get_one("NoForward") { forward = false }

        let auth_cookie = request.cookies().get_private("auth");
        match auth_cookie {
            None => {
                if forward {
                    return Outcome::Forward(());
                } else {
                    return Outcome::Failure((Status::BadRequest, AuthError::Missing));
                }
            }
            Some(cookie) => {
                let secret = settings.get_str("secret_key").unwrap();
                let token = decode(&cookie.value(), secret.as_bytes(), &Validation::default());
                match token {
                    Err(e) => {
                        eprintln!("{}",e);
                        if forward {
                            return Outcome::Forward(());
                        } else {
                            return Outcome::Failure((Status::BadRequest, AuthError::Missing));
                        }
                    }
                    Ok(token) => {
                        let auth: Auth = token.claims;
                        Outcome::Success(auth)
                    }
                }
            }
        }
    }
}

#[catch(500)]
pub fn internal_error() -> &'static str { "500" }

#[catch(404)]
pub fn not_found() -> &'static str { "404" }

#[get("/")]
pub fn dashboard(con: RedisConnection, auth: Auth) -> Template {
    let context: HashMap<&str, String> = HashMap::new();
    return Template::render("dashboard", &context);
}

#[get("/", rank=2)]
pub fn index(con: RedisConnection) -> Template {
    let context: HashMap<&str, String> = HashMap::new();
    return Template::render("index", &context);
}

#[get("/api/data")]
pub fn data(con: RedisConnection, auth: Auth) -> Json<ApiData> {
    let mut settings = config::Config::default();
    settings.merge(config::File::with_name("Settings")).unwrap();
    settings.merge(config::Environment::with_prefix("BABBLEBOT")).unwrap();
    let id: String = con.get(format!("channel:{}:id", auth.channel)).unwrap();
    let token: String = con.get(format!("channel:{}:token", auth.channel)).unwrap();

    let mut headers = header::HeaderMap::new();
    headers.insert("Accept", HeaderValue::from_str("application/vnd.twitchtv.v5+json").unwrap());
    headers.insert("Authorization", HeaderValue::from_str(&format!("OAuth {}", token)).unwrap());
    headers.insert("Client-ID", HeaderValue::from_str(&settings.get_str("client_id").unwrap()).unwrap());

    let req = reqwest::Client::builder().http1_title_case_headers().default_headers(headers).build().unwrap();
    let rsp = req.get(&format!("https://api.twitch.tv/kraken/channels/{}", id)).send();

    match rsp {
        Err(err) => {
            println!("{}", err);
            let fields: HashMap<String, String> = HashMap::new();
            let json = ApiData { fields: fields };
            return Json(json);
        }
        Ok(mut rsp) => {
            let json: Result<KrakenChannel,_> = rsp.json();
            match json {
                Err(err) => {
                    println!("{}", err);
                    let fields: HashMap<String, String> = HashMap::new();
                    let json = ApiData { fields: fields };
                    return Json(json);
                }
                Ok(json) => {
                    let mut fields: HashMap<String, String> = HashMap::new();
                    fields.insert("status".to_owned(), json.status.to_owned());
                    fields.insert("game".to_owned(), json.game.to_owned());
                    let json = ApiData { fields: fields };
                    return Json(json);
                }
            }
        }
    }
}

#[post("/api/login", data="<data>")]
pub fn login(con: RedisConnection, data: Form<ApiLoginReq>, mut cookies: Cookies) -> Json<ApiRsp> {
    let mut settings = config::Config::default();
    settings.merge(config::File::with_name("Settings")).unwrap();
    settings.merge(config::Environment::with_prefix("BABBLEBOT")).unwrap();

    let exists: bool = con.sismember("channels", data.channel.to_lowercase()).unwrap();
    if exists {
        let hashed: String = con.get(format!("channel:{}:password", data.channel.to_lowercase())).unwrap();
        let authed: bool = verify(&data.password, &hashed).unwrap();
        if authed {
            if let Ok(exp) = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
                let secret = settings.get_str("secret_key").unwrap();
                let auth = Auth { channel: data.channel.to_lowercase(), exp: exp.as_secs() + 2400000 };
                let token = encode(&Header::default(), &auth, secret.as_bytes()).unwrap();
                cookies.add_private(Cookie::new("auth", token));
            }
            let json = ApiRsp { success: true, success_value: None, field: None, error_message: None };
            return Json(json);
        } else {
            let json = ApiRsp { success: false, success_value: None, field: Some("password".to_owned()), error_message: Some("invalid password".to_owned()) };
            return Json(json);
        }
    } else {
        let json = ApiRsp { success: false, success_value: None, field: Some("channel".to_owned()), error_message: Some("channel not found".to_owned()) };
        return Json(json);
    }
}

#[post("/api/signup", data="<data>")]
pub fn signup(con: RedisConnection, mut cookies: Cookies, data: Form<ApiSignupReq>) -> Json<ApiRsp> {
    let mut settings = config::Config::default();
    settings.merge(config::File::with_name("Settings")).unwrap();
    settings.merge(config::Environment::with_prefix("BABBLEBOT")).unwrap();

    let client = reqwest::Client::new();
    let rsp = client.get("https://api.twitch.tv/helix/users").header(header::AUTHORIZATION, format!("Bearer {}", data.token)).send();

    match rsp {
        Err(e) => {
            let json = ApiRsp { success: false, success_value: None, field: Some("token".to_owned()), error_message: Some("invalid access code".to_owned()) };
            return Json(json);
        }
        Ok(mut rsp) => {
            let json: Result<HelixUsers,_> = rsp.json();
            match json {
                Err(e) => {
                    let json = ApiRsp { success: false, success_value: None, field: Some("token".to_owned()), error_message: Some("invalid access code".to_owned()) };
                    return Json(json);
                }
                Ok(json) => {
                    let exists: bool = con.sismember("channels", &json.data[0].login).unwrap();
                    if exists {
                        let json = ApiRsp { success: false, success_value: None, field: Some("token".to_owned()), error_message: Some("invalid access code".to_owned()) };
                        return Json(json);
                    } else {
                        let removed: i16 = con.lrem("invites", 1, &data.invite).unwrap();
                        if removed == 0 {
                            let json = ApiRsp { success: false, success_value: None, field: Some("invite".to_owned()), error_message: Some("invalid invite code".to_owned()) };
                            return Json(json);
                        } else {
                            let bot_name  = settings.get_str("bot_name").unwrap();
                            let bot_token = settings.get_str("bot_token").unwrap();

                            let _: () = con.sadd("bots", &bot_name).unwrap();
                            let _: () = con.sadd("channels", &json.data[0].login).unwrap();
                            let _: () = con.set(format!("bot:{}:token", &bot_name), bot_token).unwrap();
                            let _: () = con.sadd(format!("bot:{}:channels", &bot_name), &json.data[0].login).unwrap();
                            let _: () = con.set(format!("channel:{}:bot", &json.data[0].login), "babblerbot").unwrap();
                            let _: () = con.set(format!("channel:{}:token", &json.data[0].login), &data.token).unwrap();
                            let _: () = con.set(format!("channel:{}:password", &json.data[0].login), hash(&data.password, DEFAULT_COST).unwrap()).unwrap();
                            let _: () = con.set(format!("channel:{}:live", &json.data[0].login), false).unwrap();
                            let _: () = con.set(format!("channel:{}:id", &json.data[0].login), &json.data[0].id).unwrap();
                            let _: () = con.set(format!("channel:{}:display_name", &json.data[0].login), &json.data[0].display_name).unwrap();
                            let _: () = con.publish("new_channels", &json.data[0].login).unwrap();

                            if let Ok(exp) = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
                                let secret = settings.get_str("secret_key").unwrap();
                                let auth = Auth { channel: json.data[0].login.to_owned(), exp: exp.as_secs() + 2400000 };
                                let token = encode(&Header::default(), &auth, secret.as_bytes()).unwrap();
                                cookies.add_private(Cookie::new("auth", token));
                            }

                            let json = ApiRsp { success: true, success_value: None, field: None, error_message: None };
                            return Json(json);
                        }
                    }
                }
            }
        }
    }
}

#[post("/api/password", data="<data>")]
pub fn password(con: RedisConnection, data: Form<ApiPasswordReq>, auth: Auth) -> Json<ApiRsp> {
    if data.password.is_empty() {
        let json = ApiRsp { success: false, success_value: None, field: Some("password".to_owned()), error_message: Some("empty password".to_owned()) };
        return Json(json);
    } else {
        let hashed = hash(&data.password, DEFAULT_COST).unwrap();
        let _: () = con.set(format!("channel:{}:password", &auth.channel), hash(&data.password, DEFAULT_COST).unwrap()).unwrap();

        let json = ApiRsp { success: true, success_value: None, field: None, error_message: None };
        return Json(json);
    }
}

#[post("/api/title", data="<data>")]
pub fn title(con: RedisConnection, data: Form<ApiTitleReq>, auth: Auth) -> Json<ApiRsp> {
    let mut settings = config::Config::default();
    settings.merge(config::File::with_name("Settings")).unwrap();
    settings.merge(config::Environment::with_prefix("BABBLEBOT")).unwrap();
    let id: String = con.get(format!("channel:{}:id", auth.channel)).unwrap();
    let token: String = con.get(format!("channel:{}:token", auth.channel)).unwrap();

    let mut headers = header::HeaderMap::new();
    headers.insert("Accept", HeaderValue::from_str("application/vnd.twitchtv.v5+json").unwrap());
    headers.insert("Authorization", HeaderValue::from_str(&format!("OAuth {}", token)).unwrap());
    headers.insert("Client-ID", HeaderValue::from_str(&settings.get_str("client_id").unwrap()).unwrap());
    headers.insert("Content-Type", HeaderValue::from_str("application/x-www-form-urlencoded").unwrap());

    let req = reqwest::Client::builder().http1_title_case_headers().default_headers(headers).build().unwrap();
    let rsp = req.put(&format!("https://api.twitch.tv/kraken/channels/{}", id)).body(format!("channel[status]={}", data.title)).send();

    match rsp {
        Err(e) => {
            let json = ApiRsp { success: false, success_value: None, field: Some("title-field".to_owned()), error_message: Some("twitch api error".to_owned()) };
            return Json(json);
        }
        Ok(mut rsp) => {
            let json: Result<KrakenChannel,_> = rsp.json();
            match json {
                Err(e) => {
                    let json = ApiRsp { success: false, success_value: None, field: Some("title-field".to_owned()), error_message: Some("twitch api error".to_owned()) };
                    return Json(json);
                }
                Ok(json) => {
                    let json = ApiRsp { success: true, success_value: None, field: None, error_message: None };
                    return Json(json);
                }
            }
        }
    }
}

#[post("/api/game", data="<data>")]
pub fn game(con: RedisConnection, data: Form<ApiGameReq>, auth: Auth) -> Json<ApiRsp> {
    let mut settings = config::Config::default();
    settings.merge(config::File::with_name("Settings")).unwrap();
    settings.merge(config::Environment::with_prefix("BABBLEBOT")).unwrap();
    let id: String = con.get(format!("channel:{}:id", auth.channel)).unwrap();
    let token: String = con.get(format!("channel:{}:token", auth.channel)).unwrap();

    let mut headers = header::HeaderMap::new();
    headers.insert("Accept", HeaderValue::from_str("application/vnd.twitchtv.v5+json").unwrap());
    headers.insert("Authorization", HeaderValue::from_str(&format!("OAuth {}", token)).unwrap());
    headers.insert("Client-ID", HeaderValue::from_str(&settings.get_str("client_id").unwrap()).unwrap());
    headers.insert("Content-Type", HeaderValue::from_str("application/x-www-form-urlencoded").unwrap());

    let req = reqwest::Client::builder().http1_title_case_headers().default_headers(headers.clone()).build().unwrap();
    let rsp = req.get(&format!("https://api.twitch.tv/helix/games?name={}", data.game)).send();

    match rsp {
        Err(e) => {
            let json = ApiRsp { success: false, success_value: None, field: Some("game".to_owned()), error_message: Some("game not found".to_owned()) };
            return Json(json);
        }
        Ok(mut rsp) => {
            let json: Result<HelixGames,_> = rsp.json();
            match json {
                Err(e) => {
                    let json = ApiRsp { success: false, success_value: None, field: Some("game".to_owned()), error_message: Some("game not found".to_owned()) };
                    return Json(json);
                }
                Ok(json) => {
                    if json.data.len() == 0 {
                        let json = ApiRsp { success: false, success_value: None, field: Some("game".to_owned()), error_message: Some("game not found".to_owned()) };
                        return Json(json);
                    } else {
                        let name = &json.data[0].name;
                        let req = reqwest::Client::builder().http1_title_case_headers().default_headers(headers).build().unwrap();
                        let rsp = req.put(&format!("https://api.twitch.tv/kraken/channels/{}", id)).body(format!("channel[game]={}", name)).send();

                        match rsp {
                            Err(e) => {
                                let json = ApiRsp { success: false, success_value: None, field: Some("game".to_owned()), error_message: Some("twitch api error".to_owned()) };
                                return Json(json);
                            }
                            Ok(mut rsp) => {
                                let json: Result<KrakenChannel,_> = rsp.json();
                                match json {
                                    Err(e) => {
                                        let json = ApiRsp { success: false, success_value: None, field: Some("game".to_owned()), error_message: Some("twitch api error".to_owned()) };
                                        return Json(json);
                                    }
                                    Ok(json) => {
                                        let json = ApiRsp { success: true, success_value: Some(name.to_owned()), field: Some("game".to_owned()), error_message: None };
                                        return Json(json);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
