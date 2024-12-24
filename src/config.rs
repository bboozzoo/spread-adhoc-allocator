use std::io::Error;

pub struct Config {}

pub fn find() -> Result<Config, Error> {
    Err(Error::other("error"))
}
