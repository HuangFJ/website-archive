#![feature(proc_macro_hygiene, decl_macro)]
#![allow(unused_must_use)]
extern crate regex;
extern crate reqwest;
extern crate sha1;
#[macro_use]
extern crate rocket;
extern crate htmlescape;
extern crate tree_magic;
extern crate url;
#[macro_use]
extern crate log;
extern crate log4rs;

use htmlescape::decode_html;
use log::LevelFilter;
use log4rs::append::file::FileAppender;
use log4rs::config::{Appender, Config, Root};
use log4rs::encode::pattern::PatternEncoder;
use regex::Regex;
use rocket::request::{self, FromRequest, Request};
use rocket::Outcome;
use std::fs;
use std::io::prelude::*;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use url::percent_encoding::percent_decode;
use url::Url;

const FEED: &str = "http://cslabcms.nju.edu.cn/problem_solving/index.php/首页";
const INCLUDE_URL_PREFIX: [&str; 1] = ["http://cslabcms.nju.edu.cn/problem_solving/index.php/"];
const EXCLUDE_URL_PREFIX: [&str; 2] = [
    "http://cslabcms.nju.edu.cn/problem_solving/index.php/模板:",
    "http://cslabcms.nju.edu.cn/problem_solving/index.php/特殊:",
];

struct HttpRequest<'a, 'r>(&'a Request<'r>);

impl<'a, 'r> FromRequest<'a, 'r> for HttpRequest<'a, 'r> {
    type Error = ();
    fn from_request(request: &'a Request<'r>) -> request::Outcome<Self, ()> {
        Outcome::Success(HttpRequest(&request))
    }
}

impl<'a, 'r> Deref for HttpRequest<'a, 'r> {
    type Target = Request<'r>;
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

#[get("/<_path..>")]
fn index(_path: PathBuf, request: HttpRequest) -> Option<fs::File> {
    let uri = format!("{}", request.uri());
    let file = url_decode(uri.trim_start_matches('/'));
    info!("{}", file);
    fs::File::open(file).ok()
}

fn main() {
    let logfile = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{l} - {m}\n")))
        .build("output.log")
        .unwrap();

    let config = Config::builder()
        .appender(Appender::builder().build("logfile", Box::new(logfile)))
        .build(Root::builder().appender("logfile").build(LevelFilter::Info))
        .unwrap();

    log4rs::init_config(config).unwrap();

    let (sender, receiver) = mpsc::channel();
    sender.send(FEED.to_string());

    thread::spawn(move || {
        let mut fetched: Vec<String> = vec![];
        loop {
            let uri = receiver.recv().unwrap();
            if !fetched.contains(&uri) {
                fetch(&uri, &sender);
                fetched.push(uri);
            }
        }
    });

    rocket::ignite().mount("/", routes![index]).launch();
}

fn fetch(uri: &str, sender: &mpsc::Sender<String>) {
    // 根地址
    let base_url = Url::parse(FEED).unwrap().join("/").unwrap();
    // 目标地址
    let target_url = Url::parse(uri).unwrap();

    let target_url_str = url_decode(target_url.as_str());
    // 在白名单
    for in_url in INCLUDE_URL_PREFIX.iter() {
        if target_url_str.starts_with(in_url) {
            //不在黑名单
            for ex_url in EXCLUDE_URL_PREFIX.iter() {
                if target_url_str.starts_with(ex_url) {
                    return;
                }
            }
        } else {
            return;
        }
    }

    // 将url转为相对地址
    let local_uri = target_url_str.replacen(base_url.as_str(), "", 1);
    let safe_local_uri = local_uri.trim_start_matches("/");

    info!("fetching {}", safe_local_uri);

    let local_path = Path::new(safe_local_uri);
    if !local_path.exists() {
        // 本地文件不存在，网络获取内容
        let ret = reqwest::get(&target_url_str);
        let mut res = if ret.is_ok() { ret.unwrap() } else { return };

        // 创建递归目录
        fs::create_dir_all(local_path.parent().unwrap());
        // 创建文件并写入内容
        let mut local_file = match fs::File::create(local_path) {
            Ok(f) => f,
            Err(e) => {
                error!("{:?}", e);
                return;
            }
        };

        std::io::copy(&mut res, &mut local_file);
    }

    let content_type = tree_magic::from_filepath(local_path);
    info!("content type is {}", content_type);
    // 文档须是文本类型
    if !content_type.starts_with("text/") {
        return;
    }

    // 读取文档内容
    let mut content = String::new();
    fs::File::open(local_path)
        .unwrap()
        .read_to_string(&mut content)
        .unwrap();

    let mut count = 0;
    // 找到所有的href地址
    for cap in Regex::new(r#"href="([^"]+)""#)
        .unwrap()
        .captures_iter(&content)
    {
        let got = cap.get(1);
        if got.is_none() {
            return;
        }
        // 将href地址中的&编码转成正常字符串
        let href = decode_html(got.map(|x| x.as_str()).unwrap()).unwrap();

        // 剔除href为#的uri
        if !href.starts_with("#") {
            // 拼成完整的地址
            sender.send(target_url.join(&href).unwrap().to_string());
        }

        count += 1;
    }

    info!("got {} hrefs", count);
}

fn url_decode(url: &str) -> String {
    // 将url中%编码转成正常字符串
    percent_decode(url.as_bytes())
        .decode_utf8()
        .unwrap()
        .to_string()
}
