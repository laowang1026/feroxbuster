use lazy_static::lazy_static;
use regex::Regex;
use reqwest::Response;
use reqwest::Url;
use std::collections::HashSet;

/// Regular expression used in [LinkFinder](https://github.com/GerbenJavado/LinkFinder)
///
/// Incorporates change from this [Pull Request](https://github.com/GerbenJavado/LinkFinder/pull/66/files)
const LINKFINDER_REGEX: &str = r#"(?:"|')(((?:[a-zA-Z]{1,10}://|//)[^"'/]{1,}\.[a-zA-Z]{2,}[^"']{0,})|((?:/|\.\./|\./)[^"'><,;| *()(%%$^/\\\[\]][^"'><,;|()]{1,})|([a-zA-Z0-9_\-/]{1,}/[a-zA-Z0-9_\-/]{1,}\.(?:[a-zA-Z]{1,4}|action)(?:[\?|#][^"|']{0,}|))|([a-zA-Z0-9_\-/]{1,}/[a-zA-Z0-9_\-/]{3,}(?:[\?|#][^"|']{0,}|))|([a-zA-Z0-9_\-.]{1,}\.(?:php|asp|aspx|jsp|json|action|html|js|txt|xml)(?:[\?|#][^"|']{0,}|)))(?:"|')"#;

lazy_static! {
    /// `LINKFINDER_REGEX` as a regex::Regex type
    static ref REGEX: Regex = Regex::new(LINKFINDER_REGEX).unwrap();
}

/// Iterate over a given path, return a list of every sub-path found
///
/// example: `path` contains a link fragment `homepage/assets/img/icons/handshake.svg`
/// the following fragments would be returned:
///   - homepage/assets/img/icons/handshake.svg
///   - homepage/assets/img/icons/
///   - homepage/assets/img/
///   - homepage/assets/
///   - homepage/
fn get_sub_paths_from_path(path: &str) -> Vec<String> {
    log::trace!("enter: get_sub_paths_from_path({})", path);
    let mut paths = vec![];

    // filter out any empty strings caused by .split
    let mut parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    let length = parts.len();

    for _ in 0..length {
        // iterate over all parts of the path, using .pop() to remove the last part of the path
        if parts.is_empty() {
            // pop left us with an empty vector, ignore
            break;
        }

        let possible_path = parts.join("/");

        if possible_path.is_empty() {
            // .join can result in an empty string, which we don't need, ignore
            continue;
        }

        paths.push(possible_path); // good sub-path found
        parts.pop();
    }

    log::trace!("exit: get_sub_paths_from_path -> {:?}", paths);
    paths
}

/// simple helper to stay DRY, trys to join a url + fragment and add it to the `links` HashSet
fn add_link_to_set_of_links(link: &str, url: &Url, links: &mut HashSet<String>) {
    log::trace!(
        "enter: add_link_to_set_of_links({}, {}, {:?})",
        link,
        url.to_string(),
        links
    );
    match url.join(&link) {
        Ok(new_url) => {
            links.insert(new_url.to_string());
        }
        Err(e) => {
            log::error!("Could not join given url to the base url: {}", e);
        }
    }
    log::trace!("exit: add_link_to_set_of_links");
}

/// Given a `reqwest::Response`, perform the following actions
///   - parse the response's text for links using the linkfinder regex
///   - for every link found take its url path and parse each sub-path
///     - example: Response contains a link fragment `homepage/assets/img/icons/handshake.svg`
///       with a base url of http://localhost, the following urls would be returned:
///         - homepage/assets/img/icons/handshake.svg
///         - homepage/assets/img/icons/
///         - homepage/assets/img/
///         - homepage/assets/
///         - homepage/
pub async fn get_links(response: Response) -> HashSet<String> {
    log::trace!("enter: get_links({})", response.url().as_str());

    let url = response.url().clone();
    let mut links = HashSet::<String>::new();

    for capture in REGEX.captures_iter(&response.text().await.unwrap()) {
        // remove single & double quotes from both ends of the capture
        // capture[0] is the entire match, additional capture groups start at [1]
        let link = capture[0].trim_matches(|c| c == '\'' || c == '"');

        match Url::parse(link) {
            Ok(absolute) => {
                if absolute.domain() != url.domain() {
                    // domains are not the same, don't scan things that aren't part of the original
                    // target url
                    continue;
                }

                for sub_path in get_sub_paths_from_path(absolute.path()) {
                    // take a url fragment like homepage/assets/img/icons/handshake.svg and
                    // incrementally add
                    //     - homepage/assets/img/icons/
                    //     - homepage/assets/img/
                    //     - homepage/assets/
                    //     - homepage/
                    add_link_to_set_of_links(&sub_path, &url, &mut links);
                }
            }
            Err(e) => {
                // this is the expected error that happens when we try to parse a url fragment
                //     ex: Url::parse("/login") -> Err("relative URL without a base")
                // while this is technically an error, these are good results for us
                if e.to_string().contains("relative URL without a base") {
                    for sub_path in get_sub_paths_from_path(link) {
                        // incrementally save all sub-paths that led to the relative url's resource
                        add_link_to_set_of_links(&sub_path, &url, &mut links);
                    }
                } else {
                    // unexpected error has occurred
                    log::error!("Could not parse given url: {}", e);
                }
            }
        }
    }

    log::trace!("exit: get_links -> {:?}", links);
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// extract sub paths from the given url fragment; expect 4 sub paths and that all are
    /// in the expected array
    fn extractor_get_sub_paths_from_path_with_multiple_paths() {
        let path = "homepage/assets/img/icons/handshake.svg";
        let paths = get_sub_paths_from_path(&path);
        let expected = vec![
            "homepage",
            "homepage/assets",
            "homepage/assets/img",
            "homepage/assets/img/icons",
            "homepage/assets/img/icons/handshake.svg",
        ];

        assert_eq!(paths.len(), expected.len());
        for expected_path in expected {
            assert_eq!(paths.contains(&expected_path.to_string()), true);
        }
    }

    #[test]
    /// extract sub paths from the given url fragment; expect 2 sub paths and that all are
    /// in the expected array. the fragment is wrapped in slashes to ensure no empty strings are
    /// returned
    fn extractor_get_sub_paths_from_path_with_enclosing_slashes() {
        let path = "/homepage/assets/";
        let paths = get_sub_paths_from_path(&path);
        let expected = vec!["homepage", "homepage/assets"];

        assert_eq!(paths.len(), expected.len());
        for expected_path in expected {
            assert_eq!(paths.contains(&expected_path.to_string()), true);
        }
    }

    #[test]
    /// extract sub paths from the given url fragment; expect 1 sub path, no forward slashes are
    /// included
    fn extractor_get_sub_paths_from_path_with_only_a_word() {
        let path = "homepage";
        let paths = get_sub_paths_from_path(&path);
        let expected = vec!["homepage"];

        assert_eq!(paths.len(), expected.len());
        for expected_path in expected {
            assert_eq!(paths.contains(&expected_path.to_string()), true);
        }
    }

    #[test]
    /// extract sub paths from the given url fragment; expect 1 sub path, forward slash removed
    fn extractor_get_sub_paths_from_path_with_an_absolute_word() {
        let path = "/homepage";
        let paths = get_sub_paths_from_path(&path);
        let expected = vec!["homepage"];

        assert_eq!(paths.len(), expected.len());
        for expected_path in expected {
            assert_eq!(paths.contains(&expected_path.to_string()), true);
        }
    }
}
