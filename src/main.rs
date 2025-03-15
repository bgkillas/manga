use eyre::ContextCompat;
use futures::future::join_all;
use image::{GenericImage, ImageFormat, ImageReader, RgbImage};
use reqwest::header;
use reqwest::header::CONTENT_TYPE;
use std::cmp::{Ordering, PartialOrd};
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{Write, stdout};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::task;
const T: u64 = 10000;
#[tokio::main]
async fn main() -> eyre::Result<()> {
    let p1 = "/home/.li";
    let p2 = "/home/.p/";
    let p3 = "/home/.m/";
    let mut versions = HashMap::new();
    let mut stdout = stdout().lock();
    for p in fs::read_dir(p2)? {
        let p = p?.path();
        let n = p.to_str().unwrap().to_string();
        let n = n.chars().skip(p2.len()).collect::<String>();
        let r = fs::read_to_string(&p)?.trim().to_string();
        let is_list = r.starts_with('#');
        let r = r.chars().skip(1).take(5).collect::<String>();
        let major = r.chars().take(4).collect::<String>().parse::<usize>()?;
        let minor = if !is_list {
            let minor = r.chars().skip(4).collect::<String>().parse::<usize>()?;
            if minor == 0 { None } else { Some(minor) }
        } else {
            None
        };
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
            if !s.contains('-') {
                let major = s
                    .chars()
                    .skip(s.find(&name).unwrap() + name.len() + 2)
                    .collect::<String>()
                    .parse::<usize>()?;
                let ver = Version { major, minor: None };
                match last {
                    Some(v) if ver > v => {
                        last = Some(ver);
                    }
                    None => {
                        last = Some(ver);
                    }
                    _ => {}
                }
            } else if s.contains("-001") {
                let ver = s
                    .chars()
                    .skip(s.find(&name).unwrap() + name.len() + 2)
                    .take(5);
                let major = ver.clone().take(4).collect::<String>().parse::<usize>()?;
                let minor = ver.skip(4).collect::<String>().parse::<usize>()?;
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
    let mut list = fs::read_to_string(p1)?
        .lines()
        .filter_map(|l| {
            if !l.contains('#') && !l.contains("tower-of-god") {
                Some(l.chars().filter(|c| !c.is_ascii_whitespace()).collect())
            } else {
                None
            }
        })
        .collect::<Vec<String>>();
    let client = reqwest::Client::new();
    let mut mangas = Vec::new();
    let total_manga = list.len();
    while !list.is_empty() {
        let name = list.remove(0);
        let url = format!(
            "https://weebcentral.com/search/data?display_mode=Minimal+Display&limit=8&text={}",
            name.replace('-', "+")
        );
        let body = client
            .get(url)
            .header(header::REFERER, "https://weebcentral.com")
            .send()
            .await?
            .text()
            .await?;
        if body.contains("No results found") {
            list.insert(0, name);
            tokio::time::sleep(Duration::from_millis(T)).await;
            continue;
        }
        let Some(url) = body
            .lines()
            .find(|l| l.contains(&format!("/{}\" class", name)))
        else {
            tokio::time::sleep(Duration::from_millis(T)).await;
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
            tokio::time::sleep(Duration::from_millis(T)).await;
            continue;
        }
        let mut manga = Manga {
            name: name.clone(),
            chapters: Default::default(),
        };
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
            manga.chapters.insert(
                ver,
                Chapter {
                    page_count: pages,
                    url: site.clone(),
                    append: append.clone(),
                    is_list: false,
                },
            );
            if let Some(v) = versions.get(&name) {
                if v.0 >= ver {
                    break;
                }
            }
            print!(
                "\x1b[G\x1b[K{}/{}, {}/{}",
                total_manga - list.len(),
                total_manga,
                total - chapters.len(),
                total
            );
            stdout.flush()?;
            /*if chap == chapters.len() + 1 {
                while !chapters.is_empty() {
                    let base = chapters.remove(0);
                    chap -= 1;
                    let url = format!("{}/{:04}-001{}", site, chap, append);
                    if client
                        .get(url)
                        .header(header::USER_AGENT, user_agent)
                        .header(header::REFERER, "https://weebcentral.com")
                        .send()
                        .await?
                        .headers()
                        .get(CONTENT_TYPE)
                        .unwrap()
                        .to_str()?
                        != "image/png"
                    {
                        chapters.insert(0, base);
                        break;
                    }
                    manga.chapters.insert(
                        Version {
                            major: chap,
                            minor: part,
                        },
                        Chapter {
                            page_count: pages,
                            url: site.clone(),
                            append: append.clone(),
                            is_list: false,
                        },
                    );
                }
            }*/
        }
        if manga.chapters.len() > 1 {
            println!("\x1b[G\x1b[K{}: {}", manga.name, manga.chapters.len() - 1);
            mangas.push(manga);
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
    for (n, Manga { name, chapters }) in mangas.into_iter().enumerate() {
        let mut sort = Vec::new();
        for (version, chapter) in chapters {
            sort.push((version, chapter));
        }
        sort.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        if !sort.is_empty() {
            fs::create_dir_all(Path::new(p3).join(&name))?;
        }
        let l = sort.len();
        for (k, (version, chapter)) in sort.into_iter().enumerate() {
            let mut paths = Vec::new();
            print!("\x1b[G\x1b[K{}/{}, {}/{}", n + 1, total_manga, k + 1, l,);
            stdout.flush()?;
            let tasks: Vec<_> = (1..=chapter.page_count)
                .map(async |page| {
                    let client = client.clone();
                    let chapter = chapter.clone();
                    let bytes = task::spawn(async move {
                        let mut bytes: Vec<u8>;
                        loop {
                            let url = format!(
                                "{}/{:04}{}-{:03}{}",
                                chapter.url,
                                version.major,
                                version
                                    .minor
                                    .map(|i| ".".to_string() + &i.to_string())
                                    .unwrap_or(String::new()),
                                page,
                                chapter.append
                            );
                            let body = client
                                .get(url)
                                .header(header::REFERER, "https://weebcentral.com")
                                .send()
                                .await
                                .unwrap();
                            if body.headers().get(CONTENT_TYPE).unwrap().to_str().unwrap()
                                != "image/png"
                            {
                                tokio::time::sleep(Duration::from_millis(T)).await;
                                continue;
                            }
                            bytes = body.bytes().await.unwrap().into();
                            if bytes.is_empty() {
                                tokio::time::sleep(Duration::from_millis(T)).await;
                                continue;
                            }
                            break;
                        }
                        bytes
                    })
                    .await
                    .unwrap();
                    (page, bytes)
                })
                .collect();
            let images = join_all(tasks)
                .await
                .into_iter()
                .collect::<Vec<(usize, Vec<u8>)>>();
            for (page, bytes) in images {
                let path = Path::new(p3).join(&name).join(format!(
                    "1{:04}{}-{:03}",
                    version.major,
                    version.minor.unwrap_or(0),
                    page
                ));
                let mut file = File::create(&path)?;
                file.write_all(&bytes)?;
                if chapter.is_list {
                    paths.push(path);
                }
            }
            if chapter.is_list {
                let mut height = 0;
                let width = {
                    let w = ImageReader::open(&paths[paths.len() / 2])?
                        .with_guessed_format()?
                        .decode()?;
                    w.width()
                };
                let mut images = Vec::new();
                for path in &paths {
                    let w = ImageReader::open(path)?.with_guessed_format()?.decode()?;
                    if w.width() == width {
                        height += w.height();
                        images.push(w.as_rgb8().wrap_err("image err")?.clone());
                    }
                }
                for path in paths {
                    fs::remove_file(path)?;
                }
                let mut image = RgbImage::new(width, height);
                let mut running_height = 0;
                for rgb in images {
                    image.copy_from(&rgb, 0, running_height)?;
                    running_height += rgb.height();
                }
                let path = Path::new(p3)
                    .join(&name)
                    .join(format!("#{:04}", version.major));
                image.save_with_format(path, ImageFormat::Png)?;
            }
        }
    }
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
struct Manga {
    name: String,
    chapters: HashMap<Version, Chapter>,
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