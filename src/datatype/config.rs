use hyper::Url;
use rustc_serialize::Decodable;
use std::fs::File;
use std::io::ErrorKind;
use std::io::prelude::*;
use toml;

use datatype::Error;
use datatype::error::ConfigReason::{Parse, Io};
use datatype::error::ParseReason::{InvalidToml, InvalidSection};
use datatype::PackageManager;


#[derive(Default, PartialEq, Eq, Debug, Clone)]
pub struct Config {
    pub auth: AuthConfig,
    pub ota:  OtaConfig,
    pub test: TestConfig,
}

#[derive(RustcDecodable, PartialEq, Eq, Debug, Clone)]
pub struct AuthConfig {
    pub server: Url,
    pub client_id: String,
    pub secret: String
}

#[derive(RustcDecodable, PartialEq, Eq, Debug, Clone)]
pub struct OtaConfig {
    pub server: Url,
    pub vin: String,
    pub polling_interval: u64,
    pub packages_dir: String,
    pub package_manager: PackageManager,
}

#[derive(RustcDecodable, PartialEq, Eq, Debug, Clone)]
pub struct TestConfig {
    pub looping: bool,
}

impl Default for AuthConfig {
    fn default() -> AuthConfig {
        AuthConfig {
            server: Url::parse("http://127.0.0.1:9000").unwrap(),
            client_id: "client-id".to_string(),
            secret: "secret".to_string(),
        }
    }
}

impl Default for OtaConfig {
    fn default() -> OtaConfig {
        OtaConfig {
            server: Url::parse("http://127.0.0.1:8080").unwrap(),
            vin: "V1234567890123456".to_string(),
            polling_interval: 10,
            packages_dir: "/tmp/".to_string(),
            package_manager: PackageManager::Dpkg,
        }
    }
}

impl Default for TestConfig {
    fn default() -> TestConfig {
        TestConfig {
            looping: false,
        }
    }
}


pub fn parse_config(s: &str) -> Result<Config, Error> {

    fn parse_sect<T: Decodable>(tbl: &toml::Table, sect: &str) -> Result<T, Error> {
        tbl.get(sect)
            .and_then(|c| toml::decode::<T>(c.clone()) )
            .ok_or(Error::Config(Parse(InvalidSection(sect.to_string()))))
    }

    let tbl: toml::Table =
        try!(toml::Parser::new(&s)
             .parse()
             .ok_or(Error::Config(Parse(InvalidToml))));

    let auth_cfg: AuthConfig = try!(parse_sect(&tbl, "auth"));
    let ota_cfg:  OtaConfig  = try!(parse_sect(&tbl, "ota"));
    let test_cfg: TestConfig = try!(parse_sect(&tbl, "test"));

    return Ok(Config {
        auth: auth_cfg,
        ota:  ota_cfg,
        test: test_cfg,
    })
}

pub fn load_config(path: &str) -> Result<Config, Error> {

    match File::open(path) {
        Err(ref e) if e.kind() == ErrorKind::NotFound => Ok(Config::default()),
        Err(e)                                        => Err(Error::Config(Io(e))),
        Ok(mut f)                                     => {
            let mut s = String::new();
            try!(f.read_to_string(&mut s)
                 .map_err(|err| Error::Config(Io(err))));
            return parse_config(&s);
        }
    }
}


#[cfg(test)]
mod tests {

    use super::*;

    const DEFAULT_CONFIG_STRING: &'static str =
        r#"
        [auth]
        server = "http://127.0.0.1:9000"
        client_id = "client-id"
        secret = "secret"

        [ota]
        server = "http://127.0.0.1:8080"
        vin = "V1234567890123456"
        polling_interval = 10
        packages_dir = "/tmp/"
        package_manager = "dpkg"

        [test]
        looping = false
        "#;

    #[test]
    fn parse_default_config() {
        assert_eq!(parse_config(DEFAULT_CONFIG_STRING).unwrap(),
                   Config::default());
    }

    #[test]
    fn load_default_config() {
        assert_eq!(load_config("ota.toml").unwrap(),
                   Config::default());
    }

    #[test]
    fn bad_path_yields_default_config() {
        assert_eq!(load_config("").unwrap(),
                   Config::default())
    }



}
