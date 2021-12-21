// Copyright 2021 Developers of Pyroscope.

// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0>. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::error::Result;

use pprof::Report;
use std::collections::HashMap;

pub async fn pyroscope_ingest<S: AsRef<str>, N: AsRef<str>>(
    start: u64,
    sample_rate: libc::c_int,
    buffer: Vec<u8>,
    url: S,
    application_name: N,
) -> Result<()> {
    //let mut buffer = Vec::new();

            //report.fold(true, &mut buffer)?;

            if buffer.is_empty() {
                return Ok(());
            }

            let client = reqwest::Client::new();
            // TODO: handle the error of this request

            //let start: u64 = report
                //.timing
                //.start_time
                //.duration_since(std::time::UNIX_EPOCH)
                //?
                //.as_secs();

            //let new_start = std::time::SystemTime::now()
                //.duration_since(std::time::UNIX_EPOCH)
                //?
                //.as_secs() - 10u64;

            let s_start = start - start.checked_rem(10).unwrap();
            // This assumes that the interval between start and until doesn't
            // exceed 10s
            let s_until = s_start + 10;

            client
                .post(format!("{}/ingest", url.as_ref()))
                .header("Content-Type", "binary/octet-stream")
                .query(&[
                    ("name", application_name.as_ref()),
                    ("from", &format!("{}", s_start)),
                    ("until", &format!("{}", s_until)),
                    ("format", "folded"),
                    ("sampleRate", &format!("{}", sample_rate)),
                    ("spyName", "pprof-rs"),
                ])
                .body(buffer)
                .send()
                .await?;

            Ok(())
        }

pub fn merge_tags_with_app_name(application_name: String, tags: HashMap<String, String>) -> Result<String> {
    let mut tags_vec = tags
        .into_iter()
        .filter(|(k, _)| k != "__name__")
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<String>>();
    tags_vec.sort();
    let tags_str = tags_vec.join(",");

    if !tags_str.is_empty() {
        Ok(format!("{}{{{}}}", application_name, tags_str,))
    } else {
        Ok(application_name)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::utils::merge_tags_with_app_name;

    #[test]
    fn merge_tags_with_app_name_with_tags() {
        let mut tags = HashMap::new();
        tags.insert("env".to_string(), "staging".to_string());
        tags.insert("region".to_string(), "us-west-1".to_string());
        tags.insert("__name__".to_string(), "reserved".to_string());
        assert_eq!(
            merge_tags_with_app_name("my.awesome.app.cpu".to_string(), tags).unwrap(),
            "my.awesome.app.cpu{env=staging,region=us-west-1}".to_string()
        )
    }

    #[test]
    fn merge_tags_with_app_name_without_tags() {
        assert_eq!(
            merge_tags_with_app_name("my.awesome.app.cpu".to_string(), HashMap::default()).unwrap(),
            "my.awesome.app.cpu".to_string()
        )
    }
}

pub fn fold<W>(report: &Report, with_thread_name: bool, mut writer: W) -> Result<()>
where W: std::io::Write,
{
    for (key, value) in report.data.iter() {
            if with_thread_name {
                if !key.thread_name.is_empty() {
                    write!(writer, "{};", key.thread_name)?;
                } else {
                    write!(writer, "{:?};", key.thread_id)?;
                }
            }

            let last_frame = key.frames.len() - 1;
            for (index, frame) in key.frames.iter().rev().enumerate() {
                let last_symbol = frame.len() - 1;
                for (index, symbol) in frame.iter().rev().enumerate() {
                    if index == last_symbol {
                        write!(writer, "{}", symbol)?;
                    } else {
                        write!(writer, "{};", symbol)?;
                    }
                }

                if index != last_frame {
                    write!(writer, ";")?;
                }
            }

            writeln!(writer, " {}", value)?;
        }

        Ok(())
}
