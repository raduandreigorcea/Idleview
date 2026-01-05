fn main() {
    // Load .env file at build time to embed UNSPLASH_ACCESS_KEY
    if let Err(e) = dotenv::dotenv() {
        println!("cargo:warning=Failed to load .env file: {}", e);
    }
    
    // Pass UNSPLASH_ACCESS_KEY to the compiler as a build-time environment variable
    if let Ok(key) = std::env::var("UNSPLASH_ACCESS_KEY") {
        println!("cargo:rustc-env=UNSPLASH_ACCESS_KEY={}", key);
    } else {
        println!("cargo:warning=UNSPLASH_ACCESS_KEY not found in environment");
    }
    
    tauri_build::build()
}
