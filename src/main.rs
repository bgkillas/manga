use eyre::ContextCompat;
use futures::future::join_all;
use image::{GenericImage, ImageFormat, RgbImage};
use reqwest::header::CONTENT_TYPE;
use reqwest::{Client, header};
use std::cmp::{Ordering, PartialOrd};
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{Write, stdout};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::task::JoinHandle;
const T: u64 = 10000;
const TS: u64 = 1000;
#[tokio::main(flavor = "multi_thread")]
async fn main() -> eyre::Result<()> {
    let p1 = "/home/.li"; //TODO make these settings
    let p2 = "/home/.p/";
    let p3 = "/home/.m/";
    if !fs::exists(p1)? {
        fs::write(p1, Vec::new())?;
    }
    if !fs::exists(p2)? {
        fs::create_dir_all(p2)?;
    }
    if !fs::exists(p3)? {
        fs::create_dir_all(p3)?;
    }
    let mut list = fs::read_to_string(p1)?
        .lines()
        .filter_map(|l| {
            if !l.contains('#') {
                let l: String = l.chars().filter(|c| !c.is_ascii_whitespace()).collect();
                if l.contains('@') {
                    let p = l.find('@').unwrap();
                    let a = l.chars().take(p).collect::<String>();
                    let b = l.chars().skip(p + 1).collect::<String>();
                    if b.is_empty() {
                        None
                    } else {
                        Some((a, Some(b)))
                    }
                } else {
                    Some((l, None))
                }
            } else {
                None
            }
        })
        .collect::<Vec<(String, Option<String>)>>();
    let total_manga = list.len();
    let mut stdout = stdout().lock();
    print!("\x1b[G\x1b[K1/{}", total_manga);
    stdout.flush()?;
    let mut versions = HashMap::new();
    for p in fs::read_dir(p2)? {
        let p = p?.path();
        let n = p.to_str().unwrap().to_string();
        let n = n.chars().skip(p2.len()).collect::<String>();
        let r = fs::read_to_string(&p)?.trim().to_string();
        let is_list = !r.contains('-');
        let major = r.chars().take(4).collect::<String>().parse::<usize>()?;
        let minor = r
            .chars()
            .skip(4)
            .take(1)
            .collect::<String>()
            .parse::<usize>()?;
        let minor = if minor == 0 { None } else { Some(minor) };
        versions.insert(n, (Version { major, minor }, is_list));
    }
    for n in fs::read_dir(p3)? {
        let n = n?.path();
        let name = n
            .to_str()
            .unwrap()
            .chars()
            .skip(p3.len())
            .collect::<String>();
        let mut last: Option<Version> = None;
        for p in fs::read_dir(n)?
            .map(|p| p.unwrap().path())
            .collect::<Vec<PathBuf>>()
            .iter()
            .rev()
        {
            let s = p.to_str().unwrap();
            let ver = s.chars().skip(s.find(&name).unwrap() + name.len() + 1);
            let major = ver.clone().take(4).collect::<String>().parse::<usize>()?;
            let minor = ver.skip(4).take(1).collect::<String>().parse::<usize>()?;
            let minor = if minor == 0 { None } else { Some(minor) };
            let ver = Version { major, minor };
            match last {
                Some(v) if ver > v => {
                    last = Some(ver);
                }
                None => {
                    last = Some(ver);
                }
                _ => {}
            }
        }
        if let Some(ver) = last {
            match versions.get(&name) {
                Some((v, b)) if ver > *v => {
                    versions.insert(name, (ver, *b));
                }
                None => {
                    versions.insert(name, (ver, false));
                }
                _ => {}
            }
        }
    }
    let client = Client::new();
    let mut tasks: Vec<JoinHandle<eyre::Result<()>>> = Vec::new();
    while !list.is_empty() {
        let name = list.remove(0);
        let url = format!(
            "https://weebcentral.com/search/data?display_mode=Minimal+Display&limit=8&text={}",
            name.1.as_ref().unwrap_or(&name.0).replace(['-', ' '], "+")
        );
        let name = name.0;
        let body = client
            .get(url)
            .header(header::REFERER, "https://weebcentral.com")
            .send()
            .await?
            .text()
            .await?;
        if body.contains("No results found") {
            println!("\nno results found for {}", name);
            continue;
        }
        let Some(url) = body
            .lines()
            .find(|l| l.contains(&format!("/{}\" class", name)))
        else {
            println!("\nno body found for {}", name);
            continue;
        };
        let url = get_url(url)?;
        let url = url.replace(&name, "full-chapter-list");
        let body = client
            .get(url)
            .header(header::REFERER, "https://weebcentral.com")
            .send()
            .await?
            .text()
            .await?;
        let mut chapters: Vec<String> = body
            .lines()
            .filter_map(|l| {
                if l.contains("https://weebcentral.com/chapters/") {
                    Some(l.to_string())
                } else {
                    None
                }
            })
            .collect();
        if chapters.is_empty() {
            println!("\nno chapters found for {}", name);
            continue;
        }
        let mut new_chapters = Vec::new();
        let mut total_new = 0;
        let total = chapters.len();
        while !chapters.is_empty() {
            let base = chapters.remove(0);
            let url = get_url(&base)?;
            let body = client
                .get(url)
                .header(header::REFERER, "https://weebcentral.com")
                .send()
                .await?
                .text()
                .await?;
            let (Some(pages), Some(url)) = (
                body.lines().find(|l| l.contains("max_page: ")),
                body.lines().find(|l| {
                    l.contains(&name) && l.contains("href") && l.contains("as=\"image\"")
                }),
            ) else {
                if !new_chapters.is_empty() {
                    let new = std::mem::take(&mut new_chapters);
                    let client = client.clone();
                    tasks.push(tokio::spawn(download(
                        name.to_string(),
                        new,
                        p3.to_string(),
                        client,
                    )));
                }
                tokio::time::sleep(Duration::from_millis(T)).await;
                chapters.insert(0, base);
                continue;
            };
            let url = get_url(url)?;
            let pages = get_num(pages)?;
            let (site, chap, part, append) = get_chap(&url)?;
            let ver = Version {
                major: chap,
                minor: part,
            };
            new_chapters.insert(
                0,
                (
                    ver,
                    Chapter {
                        page_count: pages,
                        url: site.clone(),
                        append: append.clone(),
                        is_list: versions.get(&name).map(|(_, l)| *l).unwrap_or(false),
                    },
                ),
            );
            if let Some(v) = versions.get(&name) {
                if v.0 >= ver {
                    break;
                }
            }
            total_new += 1;
            print!(
                "\x1b[G\x1b[K{}/{}, {}/{}",
                total_manga - list.len(),
                total_manga,
                total - chapters.len(),
                total
            );
            stdout.flush()?;
        }
        if total_new > 0 {
            println!("\x1b[G\x1b[K{}: {}", name, total_new);
            if !new_chapters.is_empty() {
                let client = client.clone();
                tasks.push(tokio::spawn(download(
                    name.to_string(),
                    new_chapters,
                    p3.to_string(),
                    client,
                )));
            }
        }
        if !list.is_empty() {
            print!(
                "\x1b[G\x1b[K{}/{}",
                total_manga - list.len() + 1,
                total_manga,
            );
            stdout.flush()?;
        }
    }
    for task in join_all(tasks)
        .await
        .into_iter()
        .map(|ret| ret.unwrap())
        .collect::<Vec<eyre::Result<()>>>()
    {
        task?
    }
    print!("\x1b[G\x1b[K");
    stdout.flush()?;
    Ok(())
}
fn get_url(url: &str) -> eyre::Result<String> {
    let url = url
        .chars()
        .skip(url.find("href=\"").wrap_err("find err")? + 6)
        .collect::<String>();
    Ok(url
        .chars()
        .take(url.find('"').wrap_err("find err")?)
        .collect::<String>())
}
fn get_chap(url: &str) -> eyre::Result<(String, usize, Option<usize>, String)> {
    let mut split = url.split('/');
    let url = split.next_back().unwrap();
    let (a, b) = {
        let n = url.chars().take(url.find('-').unwrap()).collect::<String>();
        if n.contains('.') {
            let s = n.split('.').map(|s| s.to_string()).collect::<Vec<String>>();
            (s[0].clone(), Some(s[1].clone().parse::<usize>()?))
        } else {
            (n, None)
        }
    };
    Ok((
        split
            .map(|s| s.to_string())
            .collect::<Vec<String>>()
            .join("/"),
        a.parse::<usize>()?,
        b,
        url.chars()
            .skip(url.find("-001.").unwrap() + 4)
            .collect::<String>(),
    ))
}
fn get_num(url: &str) -> eyre::Result<usize> {
    let url = url
        .chars()
        .skip(url.find('\'').unwrap() + 1)
        .collect::<String>();
    Ok(url
        .chars()
        .take(url.find('\'').unwrap())
        .collect::<String>()
        .parse::<usize>()?)
}
#[derive(Eq, Hash, PartialEq, Copy, Clone)]
struct Version {
    major: usize,
    minor: Option<usize>,
}
#[derive(Clone)]
struct Chapter {
    page_count: usize,
    url: String,
    append: String,
    is_list: bool,
}
impl PartialOrd<Version> for Version {
    fn partial_cmp(&self, other: &Version) -> Option<Ordering> {
        match self.major.cmp(&other.major) {
            Ordering::Equal => match (self.minor, other.minor) {
                (Some(a), Some(b)) => a.partial_cmp(&b),
                (Some(_), None) => Some(Ordering::Greater),
                (None, Some(_)) => Some(Ordering::Less),
                (None, None) => Some(Ordering::Equal),
            },
            Ordering::Greater => Some(Ordering::Greater),
            Ordering::Less => Some(Ordering::Less),
        }
    }
}
async fn get_img(
    chapter: Chapter,
    version: Version,
    page: usize,
    client: Client,
) -> (usize, Vec<u8>) {
    let mut bytes: Vec<u8>;
    loop {
        let url = format!(
            "{}/{:04}{}-{:03}{}",
            chapter.url,
            version.major,
            version
                .minor
                .map(|i| ".".to_string() + &i.to_string())
                .unwrap_or_default(),
            page,
            chapter.append
        );
        let body = client
            .get(url)
            .header(header::REFERER, "https://weebcentral.com")
            .send()
            .await
            .unwrap();
        if body.headers().get(CONTENT_TYPE).unwrap().to_str().unwrap() != "image/png" {
            tokio::time::sleep(Duration::from_millis(TS)).await;
            continue;
        }
        bytes = body.bytes().await.unwrap().into();
        if bytes.is_empty() {
            tokio::time::sleep(Duration::from_millis(TS)).await;
            continue;
        }
        break;
    }
    (page, bytes)
}
async fn download(
    name: String,
    chapters: Vec<(Version, Chapter)>,
    p3: String,
    client: Client,
) -> eyre::Result<()> {
    let mut upper_tasks = Vec::new();
    let p = Path::new(&p3).join(&name);
    fs::create_dir_all(&p)?;
    for (version, chapter) in chapters {
        let mut paths = Vec::new();
        let tasks: Vec<_> = (1..=chapter.page_count)
            .map(async |page| {
                let client = client.clone();
                let chapter = chapter.clone();
                tokio::spawn(async move { get_img(chapter, version, page, client).await })
                    .await
                    .unwrap()
            })
            .collect();
        for (page, bytes) in join_all(tasks).await {
            if chapter.is_list {
                paths.push(bytes)
            } else {
                let path = p.join(format!(
                    "{:04}{}-{:03}",
                    version.major,
                    version.minor.unwrap_or(0),
                    page
                ));
                let mut file = File::create(&path)?;
                file.write_all(&bytes)?;
            }
        }
        if chapter.is_list {
            upper_tasks.push(tokio::spawn(convert_to_strip(
                paths,
                version,
                p3.to_string(),
                name.to_string(),
            )));
        }
    }
    for t in join_all(upper_tasks).await {
        t??;
    }
    Ok(())
}
async fn convert_to_strip(
    paths: Vec<Vec<u8>>,
    version: Version,
    p3: String,
    name: String,
) -> eyre::Result<()> {
    let mut height = 0;
    let width = {
        let w = image::load_from_memory(&paths[paths.len() / 2])?;
        w.width()
    };
    let mut images = Vec::new();
    for path in &paths {
        let w = image::load_from_memory(path)?;
        if w.width() == width {
            height += w.height();
            images.push(w.as_rgb8().wrap_err("image err")?.clone());
        }
    }
    let mut image = RgbImage::new(width, height);
    let mut running_height = 0;
    for rgb in images {
        image.copy_from(&rgb, 0, running_height)?;
        running_height += rgb.height();
    }
    let path = Path::new(&p3).join(name).join(format!(
        "{:04}{}",
        version.major,
        version.minor.unwrap_or(0)
    ));
    image.save_with_format(path, ImageFormat::Png)?;
    Ok(())
}
