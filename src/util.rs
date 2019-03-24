// urlRegex = T.pack "(((ht|f)tp(s?))\\:\\/\\/)?[a-zA-Z0-9\\-\\.]+(?<!\\.)\\.(aero|arpa|biz|cat|com|coop|edu|gov|info|jobs|mil|mobi|museum|name|net|org|pro|travel|ac|ad|ae|af|ag|ai|al|am|an|ao|ap|aq|ar|as|at|au|aw|az|ax|ba|bb|bd|be|bf|bg|bh|bi|bj|bm|bn|bo|br|bs|bt|bv|bw|by|bz|ca|cc|cd|cf|cg|ch|ci|ck|cl|cm|cn|co|cr|cs|cu|cv|cx|cy|cz|de|dj|dk|dm|do|dz|ec|ee|eg|eh|er|es|et|eu|fi|fj|fk|fm|fo|fr|ga|gb|gd|ge|gf|gg|gh|gi|gl|gm|gn|gp|gq|gr|gs|gt|gu|gw|gy|hk|hm|hn|hr|ht|hu|id|ie|il|im|in|io|iq|ir|is|it|je|jm|jo|jp|ke|kg|kh|ki|km|kn|kp|kr|kw|ky|kz|la|lb|lc|li|lk|lr|ls|lt|lu|lv|ly|ma|mc|md|mg|mh|mk|ml|mm|mn|mo|mp|mq|mr|ms|mt|mu|mv|mw|mx|my|mz|na|nc|ne|nf|ng|ni|nl|no|np|nr|nu|nz|om|pa|pe|pf|pg|ph|pk|pl|pm|pn|pr|ps|pt|pw|py|qa|re|ro|ru|rw|sa|sb|sc|sd|se|sg|sh|si|sj|sk|sl|sm|sn|so|sr|st|sv|sy|sz|tc|td|tf|tg|th|tj|tk|tl|tm|tn|to|tp|tr|tt|tv|tw|tz|ua|ug|uk|um|us|uy|uz|va|vc|ve|vg|vi|vn|vu|wf|ws|ye|yt|yu|za|zm|zw)(\\:[0-9]+)*(\\/($|[a-zA-Z0-9\\.\\,\\;\\?\\'\\\\\\+&%\\$#\\=~_\\-]+))*"

// cmdVarRegex = "\\(" ++ var ++ "((?: [\\w\\-:/!]+)*)\\)"

use crate::types::*;
use config;
use reqwest;
use reqwest::header;
use reqwest::header::HeaderValue;
use std::sync::Arc;
use r2d2_redis::r2d2;
use r2d2_redis::redis::Commands;

pub fn fetch_setting(con: Arc<r2d2::PooledConnection<r2d2_redis::RedisConnectionManager>>, channel: &str, key: &str) -> Setting {
    let res: Result<String,_> = con.hget(format!("channel:{}:settings", channel), key);
    if let Ok(setting) = res {
        Setting::String(setting)
    } else {
        let res: Result<bool,_> = con.hget(format!("channel:{}:settings", channel), key);
        if let Ok(setting) = res {
            Setting::Bool(setting)
        } else {
            Setting::Bool(false)
        }
    }
}

pub fn twitch_request_get(con: Arc<r2d2::PooledConnection<r2d2_redis::RedisConnectionManager>>, channel: &str, url: &str) -> reqwest::Result<reqwest::Response> {
    let mut settings = config::Config::default();
    settings.merge(config::File::with_name("Settings")).unwrap();
    settings.merge(config::Environment::with_prefix("BABBLEBOT")).unwrap();
    let token: String = con.get(format!("channel:{}:token", channel)).unwrap();

    let mut headers = header::HeaderMap::new();
    headers.insert("Accept", HeaderValue::from_str("application/vnd.twitchtv.v5+json").unwrap());
    headers.insert("Authorization", HeaderValue::from_str(&format!("OAuth {}", token)).unwrap());
    headers.insert("Client-ID", HeaderValue::from_str(&settings.get_str("client_id").unwrap()).unwrap());

    let req = reqwest::Client::builder().http1_title_case_headers().default_headers(headers).build().unwrap();
    let rsp = req.get(url).send();
    return rsp;
}

pub fn twitch_request_put(con: Arc<r2d2::PooledConnection<r2d2_redis::RedisConnectionManager>>, channel: &str, url: &str, body: String) -> reqwest::Result<reqwest::Response> {
    let mut settings = config::Config::default();
    settings.merge(config::File::with_name("Settings")).unwrap();
    settings.merge(config::Environment::with_prefix("BABBLEBOT")).unwrap();
    let token: String = con.get(format!("channel:{}:token", channel)).unwrap();

    let mut headers = header::HeaderMap::new();
    headers.insert("Accept", HeaderValue::from_str("application/vnd.twitchtv.v5+json").unwrap());
    headers.insert("Authorization", HeaderValue::from_str(&format!("OAuth {}", token)).unwrap());
    headers.insert("Client-ID", HeaderValue::from_str(&settings.get_str("client_id").unwrap()).unwrap());
    headers.insert("Content-Type", HeaderValue::from_str("application/x-www-form-urlencoded").unwrap());

    let req = reqwest::Client::builder().http1_title_case_headers().default_headers(headers).build().unwrap();
    let rsp = req.put(url).body(body).send();
    return rsp;
}