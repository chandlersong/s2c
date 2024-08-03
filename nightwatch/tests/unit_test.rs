use config::Config;
use serde::Deserialize;

pub fn add(left: usize, right: usize) -> usize {
    left + right
}




#[cfg(test)]
mod tests {
    use super::*;

    #[ignore]
    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }

    #[test]
    fn test_load_config() {

        let settings = Config::builder()
            // Add in `./Settings.toml`
            .add_source(config::File::with_name("conf/Settings"))
            // Add in settings from the environment (with a prefix of APP)
            // Eg.. `APP_DEBUG=1 ./target/app` would set the `debug` key
            .add_source(config::Environment::with_prefix("APP"))
            .build()
            .unwrap();

        // Print out our settings (as a HashMap)
        println!(
            "{:?}",
            settings
                .try_deserialize::Vec<<HashMap<String, String>>()
                .unwrap()
        );
    }
}
