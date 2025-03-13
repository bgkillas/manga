use reqwest::header;
use reqwest::header::CONTENT_TYPE;
use std::cmp::{Ordering, PartialOrd};
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
fn get_url(url: &str) -> String {
    let url = url
        .chars()
        .skip(url.find("href=\"").unwrap() + 6)
        .collect::<String>();
    url.chars().take(url.find('"').unwrap()).collect::<String>()
}
fn get_chap(url: &str) -> eyre::Result<(String, usize, Option<usize>, String)> {
    let split = url.split('/');
    let n = split.clone().count();
    let url = split.clone().last().unwrap();
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
            .take(n - 1)
            .map(|s| s.to_string())
            .collect::<Vec<String>>()
            .join("/"),
        a.parse::<usize>()?,
        b,
        url.chars().skip(url.find("-001.").unwrap() + 4).collect::<String>(),
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
const T: u64 = 10000;
struct Manga {
    name: String,
    chapters: HashMap<Version, Chapter>,
}
#[derive(Eq, Hash, PartialEq)]
struct Version {
    major: usize,
    minor: Option<usize>,
}
struct Chapter {
    page_count: usize,
    url: String,
    append: String,
    is_list: bool,
}
impl PartialOrd<Version> for Version {
    fn partial_cmp(&self, other: &Version) -> Option<Ordering> {
        if self.major == other.major {
            match (self.minor, other.minor) {
                (Some(a), Some(b)) => a.partial_cmp(&b),
                (Some(_), None) => Some(Ordering::Greater),
                (None, Some(_)) => Some(Ordering::Less),
                (None, None) => Some(Ordering::Equal),
            }
        } else if self.major > other.major {
            Some(Ordering::Greater)
        } else {
            Some(Ordering::Less)
        }
    }
}
#[tokio::main]
async fn main() -> eyre::Result<()> {
    let p1 = "/home/.li";
    let p2 = "/home/.p/";
    let p3 = "/home/.m/";
    let mut list = fs::read_to_string(p1)?
        .lines()
        .take(1)
        .filter_map(|l| {
            if !l.contains('#') && !l.contains("tower-of-god") {
                Some(l.chars().filter(|c| !c.is_ascii_whitespace()).collect())
            } else {
                None
            }
        })
        .collect::<Vec<String>>();
    let client = reqwest::Client::new();
    let user_agent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/113.0.5672.127 Safari/537.36";
    let mut mangas = Vec::new();
    while !list.is_empty() {
        let name = list.remove(0);
        let url = format!(
            "https://weebcentral.com/search/data?display_mode=Minimal+Display&limit=8&text={}",
            name.replace('-', "+")
        );
        let body = client
            .get(url)
            .header(header::USER_AGENT, user_agent)
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
        let url = get_url(url);
        let url = url.replace(&name, "full-chapter-list");
        let body = client
            .get(url)
            .header(header::USER_AGENT, user_agent)
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
        while !chapters.is_empty() {
            println!("{} {}", list.len(), chapters.len());
            let base = chapters.remove(0);
            let url = get_url(&base);
            let body = client
                .get(url)
                .header(header::USER_AGENT, user_agent)
                .header(header::REFERER, "https://weebcentral.com")
                .send()
                .await?
                .text()
                .await?;
            let (Some(pages), Some(url)) = (
                body.lines().find(|l| l.contains("max_page: ")),
                body.lines()
                    .find(|l| l.contains(&name) && l.contains("image")),
            ) else {
                tokio::time::sleep(Duration::from_millis(T)).await;
                chapters.insert(0, base);
                continue;
            };
            let url = get_url(url);
            let pages = get_num(pages)?;
            let (site, chap, part, append) = get_chap(&url)?;
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
            /*if chap == chapters.len() + 1 {
                while !chapters.is_empty() {
                    println!("{} {}", list.len(), chapters.len());
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
        mangas.push(manga);
    }
    let mut versions = HashMap::new();
    for p in fs::read_dir(p2)? {
        let p = p?.path();
        let n = p.to_str().unwrap().to_string();
        let n = n.chars().skip(p2.len()).collect::<String>();
        let r = fs::read_to_string(p)?.trim().to_string();
        if r.len() < 10 {
            continue;
        }
        let is_list = r.starts_with('#');
        let r = if is_list {
            r.chars().skip(1)
        } else {
            r.chars().skip(0)
        }
        .skip(1)
        .take(5)
        .collect::<String>();
        let major = r.chars().take(4).collect::<String>().parse::<usize>()?;
        let minor = r.chars().skip(4).collect::<String>().parse::<usize>()?;
        let minor = if minor == 0 { None } else { Some(minor) };
        versions.insert(n, (Version { major, minor }, is_list));
    }
    for Manga { name, chapters } in mangas {
        let chapters = if let Some((read, lst)) = versions.get(&name) {
            let mut new = HashMap::new();
            for (version, mut chapter) in chapters {
                if version >= *read {
                    if !fs::exists(Path::new(p3).join(&name).join(format!(
                        "1{:04}{}-{:03}",
                        version.major,
                        version.minor.unwrap_or(0),
                        chapter.page_count,
                    )))? {
                        chapter.is_list = *lst;
                        new.insert(version, chapter);
                    }
                }
            }
            new
        } else {
            chapters
        };
        let mut first = true;
        let mut sort = Vec::new();
        for (version, chapter) in chapters {
            sort.push((version, chapter));
        }
        sort.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        for (version, chapter) in sort {
            let mut page = 1;
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
                println!("{}", url);
                if first {
                    fs::create_dir_all(Path::new(p3).join(&name))?;
                    first = false
                }
                let mut file = File::create(Path::new(p3).join(&name).join(format!(
                    "1{:04}{}-{:03}",
                    version.major,
                    version.minor.unwrap_or(0),
                    page,
                )))?;
                let body = client
                    .get(url)
                    .header(header::USER_AGENT, user_agent)
                    .header(header::REFERER, "https://weebcentral.com")
                    .send()
                    .await?;
                if body.headers().get(CONTENT_TYPE).unwrap().to_str()? != "image/png" {
                    tokio::time::sleep(Duration::from_millis(T)).await;
                    continue;
                }
                let bytes = body.bytes().await?;
                if bytes.is_empty() {
                    tokio::time::sleep(Duration::from_millis(T)).await;
                    continue;
                }
                file.write_all(&bytes)?;
                page += 1;
                if page == chapter.page_count {
                    break
                }
            }
        }
    }
    Ok(())
}