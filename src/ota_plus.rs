use hyper::Url;
use hyper::header::{Authorization, Bearer, ContentType};
use hyper::mime::{Mime, TopLevel, SubLevel, Attr, Value};
use rustc_serialize::json;
use std::fs::File;
use std::path::PathBuf;
use std::result::Result;

use datatype::AccessToken;
use datatype::Config;
use datatype::Error;
use datatype::error::OtaReason::{CreateFile, Client};
use datatype::Package;
use datatype::UpdateRequestId;
use datatype::{UpdateReport, UpdateReportWithVin};
use http_client::{HttpClient, HttpRequest};


fn vehicle_endpoint(config: &Config, s: &str) -> Result<Url, Error> {
    Ok(try!(config.ota.server.join(&format!("/api/v1/vehicles/{}{}", config.auth.vin, s))))
}

pub fn download_package_update<C: HttpClient>(token:  &AccessToken,
                                              config: &Config,
                                              id:     &UpdateRequestId) -> Result<PathBuf, Error> {

    let url = try!(vehicle_endpoint(config, &format!("/updates/{}/download", id)));
    let req = HttpRequest::get(url)
        .with_header(Authorization(Bearer { token: token.access_token.clone() }));

    let mut path = PathBuf::new();
    path.push(&config.ota.packages_dir);
    path.push(id);
    path.set_extension(config.ota.package_manager.extension());

    let file = try!(File::create(path.as_path())
                    .map_err(|e| Error::Ota(CreateFile(path.clone(), e))));

    try!(C::new().send_request_to(&req, file)
         .map_err(|e| Error::Ota(Client(req.to_string(), format!("{}", e)))));

    return Ok(path)
}

pub fn send_install_report<C: HttpClient>(token:  &AccessToken,
                                          config: &Config,
                                          report: &UpdateReport) -> Result<(), Error> {

    let report_with_vin = UpdateReportWithVin::new(&config.auth.vin, &report);
    let json = try!(json::encode(&report_with_vin)
                    .map_err(|_| Error::ParseError(String::from("JSON encoding error"))));

    let url = try!(vehicle_endpoint(config, &format!("/updates/{}", report.update_id)));
    let req = HttpRequest::post(url)
        .with_header(Authorization(Bearer { token: token.access_token.clone() }))
        .with_header(ContentType(Mime(
            TopLevel::Application,
            SubLevel::Json,
            vec![(Attr::Charset, Value::Utf8)])))
        .with_body(&json);

    let _: String = try!(C::new().send_request(&req));

    return Ok(())
}

pub fn get_package_updates<C: HttpClient>(token:  &AccessToken,
                                          config: &Config) -> Result<Vec<UpdateRequestId>, Error> {

    let url = try!(vehicle_endpoint(&config, "/updates"));
    let req = HttpRequest::get(url)
        .with_header(Authorization(Bearer { token: token.access_token.clone() }));

    let body = try!(C::new().send_request(&req)
                    .map_err(|e| Error::ClientError(format!("Can't consult package updates: {}", e))));

    return Ok(try!(json::decode::<Vec<UpdateRequestId>>(&body)));

}

pub fn post_packages<C: HttpClient>(token:  &AccessToken,
                                    config: &Config,
                                    pkgs:   &Vec<Package>) -> Result<(), Error> {

    let json = try!(json::encode(&pkgs)
                    .map_err(|_| Error::ParseError(String::from("JSON encoding error"))));

    let url = try!(vehicle_endpoint(config, "/updates"));
    let req = HttpRequest::post(url)
        .with_header(Authorization(Bearer { token: token.access_token.clone() }))
        .with_header(ContentType(Mime(
            TopLevel::Application,
            SubLevel::Json,
            vec![(Attr::Charset, Value::Utf8)])))
        .with_body(&json);

    let _: String = try!(C::new().send_request(&req));

    return Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use datatype::AccessToken;
    use datatype::{Config, OtaConfig};
    use datatype::Package;
    use http_client::{MockHttpClient, BadHttpClient};
    use http_client::HttpClient;

    fn test_token() -> AccessToken {
        AccessToken {
            access_token: "token".to_string(),
            token_type: "bar".to_string(),
            expires_in: 20,
            scope: vec![]
        }
    }

    fn test_package() -> Package {
        Package {
            name: "hey".to_string(),
            version: "1.2.3".to_string()
        }
    }

    #[test]
    fn test_post_packages_sends_authentication() {
        assert_eq!(
            post_packages::<MockHttpClient>(&test_token(), &Config::default(), &vec![test_package()])
                .unwrap(), ())
    }

    #[test]
    fn test_get_package_updates() {
        assert_eq!(get_package_updates::<MockHttpClient>(&test_token(), &Config::default()).unwrap(),
                   vec!["pkgid".to_string()])
    }

    #[test]
    #[ignore] // TODO: docker daemon requires user namespaces for this to work
    fn bad_packages_dir_download_package_update() {
        let mut config = Config::default();
        config.ota = OtaConfig { packages_dir: "/".to_string(), .. config.ota };

        assert_eq!(
            format!("{}",
                    download_package_update::<MockHttpClient>(&test_token(), &config, &"0".to_string())
                    .unwrap_err()),
            r#"Ota error, failed to create file "/0.deb": Permission denied (os error 13)"#)
    }

    #[test]
    fn bad_client_download_package_update() {
        assert_eq!(
            format!("{}",
                    download_package_update::<BadHttpClient>
                    (&test_token(), &Config::default(), &"0".to_string())
                    .unwrap_err()),
            r#"Ota error, the request: GET http://127.0.0.1:8080/api/v1/vehicles/V1234567890123456/updates/0/download,
results in the following error: bad client."#)
    }
}
