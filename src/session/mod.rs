pub mod schroot;

pub enum Error {
    CalledProcessError(u8)
}

pub trait Session {
    fn check_output(
        &self,
        argv: Vec<&str>,
        cwd: Option<&str>,
        user: Option<&str>,
        env: Option<HashMap<String, String>>,
    ) -> Result<Vec<u8>, Error>;

}

pub fn which(session: &impl Session, name: &str) -> Option<String> {
    let ret = match session.check_output(vec!["which", name], Some("/"), None, None) {
        Ok(ret) => ret,
        Err(Error::CalledProcessError(1)) => return None,
        Err(e) => panic!("Unexpected error: {:?}", e),
    };
    if ret.is_empty() {
        None
    } else {
        Some(String::from_utf8(ret).unwrap().trim().to_string())
    }
}

pub fn get_user(session: &Session) -> String {
    String::from_utf8(
    session.check_output(vec!["sh", "-c", "echo $USER"], Some("/"), None, None).unwrap())
        .trim()
}
