use crate::util::*;
use crate::commands::*;
use std::{thread,mem};
use std::collections::{HashMap,HashSet};
use serenity::client::{Context, EventHandler};
use serenity::model::channel::Message;
use serde::{Serialize, Deserialize};
use serde_json::Value;
use regex::{Regex,Captures,escape};
use crossbeam_channel::{Sender,Receiver};
use redis::from_redis_value;
use rocket_contrib::database;
use rocket_contrib::databases::redis;
use reqwest::r#async::Decoder;
use futures::stream::Stream;
use futures::future::{Future,join_all};

pub struct DiscordHandler {
    pub channel: String,
    pub db: (Sender<Vec<String>>, Receiver<Result<redis::Value, String>>)
}

impl EventHandler for DiscordHandler {
    fn message(&self, ctx: Context, msg: Message) {
        let db = self.db.clone();
        let id: String = from_redis_value(&redis_call(db.clone(), vec!["hget", &format!("channel:{}:settings", self.channel), "discord:mod-channel"]).unwrap_or(redis::Value::Data("".as_bytes().to_owned()))).unwrap();
        if msg.channel_id.as_u64().to_string() == id {
            let rgx = Regex::new("<:(\\w+):\\d+>").unwrap();
            let content = rgx.replace_all(&msg.content, |caps: &Captures| { if let Some(emote) = caps.get(1) { emote.as_str().to_owned() } else { "".to_owned() } }).to_string();
            redis_call(db.clone(), vec!["publish", &format!("channel:{}:signals:command", &self.channel), &content]);
        } else {
            let mut words = msg.content.split_whitespace();
            if let Some(word) = words.next() {
                let word = word.to_lowercase();
                let args: Vec<String> = words.map(|w| w.to_owned()).collect();
                // TODO: expand aliases
                let res: Result<redis::Value,_> = redis_call(db.clone(), vec!["hget", &format!("channel:{}:commands:{}", self.channel, word), "message"]);
                if let Ok(value) = res {
                    let mut message: String = from_redis_value(&value).unwrap();
                    for var in command_vars.iter() {
                        message = parse_var(var, &message, None, self.channel.clone(), None, args.clone(), db.clone());
                    }

                    let mut futures = Vec::new();
                    let mut regexes: Vec<String> = Vec::new();
                    for var in command_vars_async.iter() {
                        let rgx = Regex::new(&format!("\\({} ?((?:[\\w\\-\\?\\._:/&!= ]+)*)\\)", var.0)).unwrap();
                        for captures in rgx.captures_iter(&message) {
                            if let (Some(capture), Some(vargs)) = (captures.get(0), captures.get(1)) {
                                let vargs: Vec<String> = vargs.as_str().split_whitespace().map(|str| str.to_owned()).collect();
                                if let Some((builder, func)) = (var.1)(None, self.channel.clone(), None, vargs.clone(), args.clone(), db.clone()) {
                                    let db = db.clone();
                                    let channel = self.channel.clone();
                                    let future = builder.send().and_then(|mut res| { (Ok(channel), Ok(db), mem::replace(res.body_mut(), Decoder::empty()).concat2()) }).map(func);
                                    futures.push(future);
                                    regexes.push(capture.as_str().to_owned());
                                }
                            }
                        }
                    }

                    thread::spawn(move || {
                        let mut core = tokio_core::reactor::Core::new().unwrap();
                        let work = join_all(futures);
                        for (i,res) in core.run(work).unwrap().into_iter().enumerate() {
                            let rgx = Regex::new(&escape(&regexes[i])).unwrap();
                            message = rgx.replace(&message, |_: &Captures| { &res }).to_string();
                        }
                        let _ = msg.channel_id.say(&ctx.http, message);
                    });
                }
            }
        }
    }
}

#[database("redis")]
pub struct RedisConnection(redis::Connection);

#[derive(Debug)]
pub enum RedisResult {
    String,
    Hash,
    Vec
}

#[derive(Debug)]
pub enum AuthError {
    Missing,
    Invalid
}

#[derive(Debug)]
pub enum ThreadAction {
    Kill,
    Part(String)
}

#[derive(Debug)]
pub enum ClientAction {
    Command(String, Vec<String>),
    Privmsg(String, String),
    Parsed(String, String),
    Part(String)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscordOpCode {
    pub op: u16,
    pub d: Value
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscordPayload {
    pub heartbeat_inverval: i32
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Auth {
    pub channel: String,
    pub exp: u64
}

#[derive(Debug, Deserialize)]
pub struct TmiChatters {
    pub chatters: Chatters
}

#[derive(Debug, Deserialize)]
pub struct Chatters {
    pub moderators: Vec<String>,
    pub viewers: Vec<String>,
    pub vips: Vec<String>
}

#[derive(Debug, Deserialize)]
pub struct TwitchErr {
    pub status: u8,
    pub error: String,
    pub message: String
}

#[derive(Debug, Deserialize)]
pub struct TwitchRsp {
    pub access_token: String,
    pub token_type: String,
    pub scope: Vec<String>,
    pub refresh_token: String
}

#[derive(Debug, Deserialize)]
pub struct TwitchRefresh {
    pub access_token: String,
    pub refresh_token: String
}

#[derive(Debug, Deserialize)]
pub struct SpotifyRsp {
    pub access_token: String,
    pub token_type: String,
    pub scope: String,
    pub refresh_token: String
}

#[derive(Debug, Deserialize)]
pub struct SpotifyRefresh {
    pub access_token: String,
    pub token_type: String,
    pub scope: String
}

#[derive(Debug, Deserialize)]
pub struct PatreonRsp {
    pub access_token: String,
    pub token_type: String,
    pub scope: String,
    pub refresh_token: String
}

#[derive(Debug, Deserialize)]
pub struct PatreonIdentity {
    pub data: PatreonIdentityData,
    pub included: Vec<PatreonIdentityIncluded>
}

#[derive(Debug, Deserialize)]
pub struct PatreonIdentityIncluded {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub relationships: Option<PatreonIdentityIncludedRelationships>
}

#[derive(Debug, Deserialize)]
pub struct PatreonIdentityIncludedRelationships {
    pub campaign: PatreonIdentityIncludedCampaign
}

#[derive(Debug, Deserialize)]
pub struct PatreonIdentityIncludedCampaign {
    pub data: PatreonIdentityIncludedData
}

#[derive(Debug, Deserialize)]
pub struct PatreonIdentityIncludedData {
    pub id: String
}

#[derive(Debug, Deserialize)]
pub struct PatreonIdentityData {
    pub relationships: PatreonIdentityRelationships
}

#[derive(Debug, Deserialize)]
pub struct PatreonIdentityRelationships {
    pub memberships: PatreonIdentityMemberships
}

#[derive(Debug, Deserialize)]
pub struct PatreonIdentityMemberships {
    pub data: Vec<PatreonIdentityMembershipsData>
}

#[derive(Debug, Deserialize)]
pub struct PatreonIdentityMembershipsData {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String
}

#[derive(Debug, Deserialize)]
pub struct HelixClips {
    pub data: Vec<HelixClip>
}

#[derive(Debug, Deserialize)]
pub struct HelixClip {
    pub id: String
}

#[derive(Debug, Deserialize)]
pub struct HelixUsers {
    pub data: Vec<HelixUsersData>
}

#[derive(Debug, Deserialize)]
pub struct HelixUsersData {
    pub id: String,
    pub login: String,
    pub display_name: String
}

#[derive(Debug, Deserialize)]
pub struct HelixGames {
    pub data: Vec<HelixGamesData>
}

#[derive(Debug, Deserialize)]
pub struct HelixGamesData {
    pub name: String
}

#[derive(Debug, Deserialize)]
pub struct KrakenHosts {
    pub hosts: Vec<KrakenHost>
}

#[derive(Debug, Deserialize)]
pub struct KrakenHost {
    pub host_id: String,
    pub target_id: String
}

#[derive(Debug, Deserialize)]
pub struct KrakenFollow {
    pub created_at: String
}

#[derive(Debug, Deserialize)]
pub struct KrakenFollows {
    #[serde(rename = "_total")]
    pub total: i32
}

#[derive(Debug, Deserialize)]
pub struct KrakenSubs {
    #[serde(rename = "_total")]
    pub total: i32
}

#[derive(Debug, Deserialize)]
pub struct KrakenUsers {
    #[serde(rename = "_total")]
    pub total: i32,
    pub users: Vec<KrakenUser>
}

#[derive(Debug, Deserialize)]
pub struct KrakenUser {
    #[serde(rename = "_id")]
    pub id: String,
    pub name: String,
    pub created_at: String
}

#[derive(Debug, Deserialize)]
pub struct KrakenChannel {
    pub status: String,
    pub game: String,
    pub name: String,
    pub logo: String,
    pub url: String,
    pub display_name: String
}

#[derive(Debug, Deserialize)]
pub struct KrakenStreams {
    pub streams: Vec<KrakenStream>
}

#[derive(Debug, Deserialize)]
pub struct KrakenStream {
    pub created_at: String,
    pub viewers: i32,
    pub channel: KrakenChannel
}

#[derive(Debug, Deserialize)]
pub struct SpotifyPlaying {
    pub item: Option<SpotifyItem>
}

#[derive(Debug, Deserialize)]
pub struct SpotifyItem {
    pub name: String,
    pub album: SpotifyAlbum,
    pub artists: Vec<SpotifyArtist>
}

#[derive(Debug, Deserialize)]
pub struct SpotifyAlbum {
    pub name: String
}

#[derive(Debug, Deserialize)]
pub struct SpotifyArtist {
    pub name: String
}

#[derive(Debug, Deserialize)]
pub struct FortniteApi {
    pub stats: FortniteStats,
    pub lifeTimeStats: Vec<FortniteLifeStat>,
    pub recentMatches: Vec<FortniteMatch>
}

#[derive(Debug, Deserialize)]
pub struct FortniteLifeStat {
    pub key: String,
    pub value: String
}

#[derive(Debug, Deserialize)]
pub struct FortniteStats {
    #[serde(rename = "p2")]
    pub solo: HashMap<String,FortniteStat>,
    #[serde(rename = "p10")]
    pub duo: HashMap<String,FortniteStat>,
    #[serde(rename = "p9")]
    pub squad: HashMap<String,FortniteStat>
}

#[derive(Debug, Deserialize)]
pub struct FortniteStat {
    pub value: String
}

#[derive(Debug, Deserialize)]
pub struct FortniteMatch {
    pub id: i64,
    pub playlist: String,
    pub kills: i16,
    pub top1: i16
}

#[derive(Debug, Deserialize)]
pub struct PubgMatch {
    pub data: PubgMatchData,
    pub included: Vec<PubgMatchIncluded>
}

#[derive(Debug, Deserialize)]
pub struct PubgMatchData {
    pub attributes: PubgMatchAttrs
}

#[derive(Debug, Deserialize)]
pub struct PubgMatchAttrs {
    #[serde(rename = "createdAt")]
    pub created_at: String
}

#[derive(Debug, Deserialize)]
pub struct PubgMatchIncluded {
    #[serde(rename = "type")]
    pub type_: String,
    pub attributes: Value
}

#[derive(Debug, Deserialize)]
pub struct PubgPlayers {
    pub data: Vec<Player>
}

#[derive(Debug, Deserialize)]
pub struct Player {
    pub id: String
}

#[derive(Debug, Deserialize)]
pub struct PubgPlayer {
    pub data: PlayerData
}

#[derive(Debug, Deserialize)]
pub struct PlayerData {
    pub relationships: PlayerRelationships
}

#[derive(Debug, Deserialize)]
pub struct PlayerRelationships {
    pub matches: PlayerMatches
}

#[derive(Debug, Deserialize)]
pub struct PlayerMatches {
    pub data: Vec<PlayerMatch>
}

#[derive(Debug, Deserialize)]
pub struct PlayerMatch {
    pub id: String,
    #[serde(rename = "type")]
    pub mtype: String
}

#[derive(Debug, Deserialize)]
pub struct YoutubeData {
    pub title: String
}

#[derive(Debug, Serialize)]
pub struct ApiData {
    pub channel: String,
    pub state: String,
    pub fields: HashMap<String, String>,
    pub commands: HashMap<String, String>,
    pub notices: HashMap<String, Vec<String>>,
    pub settings: HashMap<String, String>,
    pub blacklist: HashMap<String, HashMap<String,String>>,
    pub keywords: HashMap<String, HashMap<String,String>>,
    pub songreqs: Vec<(String,String,String)>,
    pub integrations: HashMap<String, HashMap<String,String>>
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiReady {
    pub success: bool
}

#[derive(Serialize, Deserialize)]
pub struct ApiRedisReq {
    pub args: Vec<String>,
    pub secret_key: String
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiRedisExecute {
    pub success: bool,
    pub message: String
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiRedisString {
    pub success: bool,
    pub message: String,
    pub result: String
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiRedisVec {
    pub success: bool,
    pub message: String,
    pub result: Vec<String>
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiRedisHash {
    pub success: bool,
    pub message: String,
    pub result: HashSet<String>
}

#[derive(Serialize)]
pub struct ApiRsp {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>
}

#[derive(Serialize, Deserialize)]
pub struct LocalRsp {
    pub version: String,
    pub success: bool,
    pub actions: Vec<LocalAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>
}

#[derive(Serialize, Deserialize)]
pub struct LocalAction {
    pub name: String,
    pub keys: Vec<u16>,
    pub hold: u64,
    pub delay: u64
}

#[derive(Serialize, FromForm)]
pub struct ApiLocalReq {
    pub channel: String,
    pub key: String
}

#[derive(FromForm)]
pub struct ApiLogsReq {
    pub num: String
}

#[derive(Serialize, FromForm)]
pub struct ApiLoginReq {
    pub channel: String,
    pub password: String
}

#[derive(FromForm)]
pub struct ApiSignupReq {
    pub token: String,
    pub refresh: String,
    pub password: String
}

#[derive(FromForm)]
pub struct ApiPasswordReq {
    pub password: String
}

#[derive(FromForm)]
pub struct ApiTitleReq {
    pub title: String
}

#[derive(FromForm)]
pub struct ApiGameReq {
    pub game: String
}

#[derive(FromForm)]
pub struct ApiSaveCommandReq {
    pub command: String,
    pub message: String
}

#[derive(FromForm)]
pub struct ApiTrashCommandReq {
    pub command: String
}

#[derive(FromForm)]
pub struct ApiNoticeReq {
    pub interval: String,
    pub command: String
}

#[derive(FromForm)]
pub struct ApiSaveSettingReq {
    pub name: String,
    pub value: String
}

#[derive(FromForm)]
pub struct ApiTrashSettingReq {
    pub name: String
}

#[derive(FromForm)]
pub struct ApiNewBlacklistReq {
    pub regex: String,
    pub length: String
}

#[derive(FromForm)]
pub struct ApiSaveBlacklistReq {
    pub regex: String,
    pub length: String,
    pub key: String
}

#[derive(FromForm)]
pub struct ApiTrashBlacklistReq {
    pub key: String
}

#[derive(FromForm)]
pub struct ApiNewKeywordReq {
    pub regex: String,
    pub command: String
}

#[derive(FromForm)]
pub struct ApiSaveKeywordReq {
    pub regex: String,
    pub command: String,
    pub key: String
}

#[derive(FromForm)]
pub struct ApiTrashKeywordReq {
    pub key: String
}

#[derive(FromForm)]
pub struct ApiTrashSongReq {
    pub index: usize
}
