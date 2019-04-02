#![feature(proc_macro_hygiene, decl_macro)]
#![allow(unused_must_use)]
extern crate regex;
extern crate reqwest;
extern crate sha1;
#[macro_use]
extern crate rocket;
extern crate tree_magic;
extern crate url;

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

const FEED: &str = "http://cslabcms.nju.edu.cn/problem_solving/index.php/%E9%A6%96%E9%A1%B5";
const TARGET_PATH: &str="http://cslabcms.nju.edu.cn/problem_solving/index.php/";

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
    println!("{}", file);
    fs::File::open(file).ok()
}

fn main() {
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

    rocket::ignite().mount(TARGET_PATH, routes![index]).launch();
}

fn fetch(uri: &str, sender: &mpsc::Sender<String>) {
    let base_url = Url::parse(FEED).unwrap().join("/").unwrap();
    let target_url = Url::parse(uri).unwrap();

    // 将url转为相对地址
    let local_uri = if target_url.as_str().starts_with(TARGET_PATH) {
        url_decode(
            target_url
                .as_str()
                .replacen(base_url.as_str(), "", 1)
                .trim_start_matches("/"),
        )
    } else {
        return;
    };

    println!("fetching {}", local_uri);

    let local_path = Path::new(&local_uri);
    if !local_path.exists() {
        // 本地文件不存在，网络获取内容
        let ret = reqwest::get(target_url.as_str());
        let mut res = if ret.is_ok() { ret.unwrap() } else { return };

        // 创建递归目录
        fs::create_dir_all(local_path.parent().unwrap());
        // 创建文件并写入内容
        let mut local_file = match fs::File::create(local_path) {
            Ok(f) => f,
            Err(e) => {
                println!("{:?}", e);
                return;
            }
        };

        std::io::copy(&mut res, &mut local_file);
    }

    let content_type = tree_magic::from_filepath(local_path);
    println!("content type is {}", content_type);
    if !content_type.starts_with("text/") {
        return;
    }

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
        let href = got.map(|x| x.as_str()).unwrap();

        // 剔除href为#的uri
        if !href.starts_with("#") {
            sender.send(target_url.join(href).unwrap().to_string());
        }

        count += 1;
    }

    println!("got {} hrefs", count);
}

fn url_decode(url: &str) -> String {
    // 将url中%的字符串解码成正常字符串
    percent_decode(url.as_bytes())
        .decode_utf8()
        .unwrap()
        .to_string()
}
