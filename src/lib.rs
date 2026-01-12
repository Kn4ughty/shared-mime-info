// https://specifications.freedesktop.org/shared-mime-info/0.21/ar01s02.html

// use crate::mime_cache;
pub mod mime_cache;

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
