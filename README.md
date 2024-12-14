# Usage
Include our drone by adding to your Cargo.toml:
```toml
[dependencies]
ap2024_unitn_cppenjoyers_drone = { git = "https://github.com/Cpp-enjoyers/drone.git" }
```

And then import it in your code:
```rust
use ap2024_unitn_cppenjoyers_drone;
...

fn main(){

  let drone = ap2024_unitn_cppenjoyers_drone::CppEnjoyersDrone::new(...);

}

```
# Configuration
There are several settings that you can adjust to customize your experience with our drone.
## Logging
If you can't see the logs, be sure to enable the environment variable "RUST_LOG" to your desired level.
By default, the program logs everything in debug mode and only errors in release mode.

## RingBuffer for FloodIDs
By default, the drone keeps track of the last 64 FloodIDs for every different initiator. If you want to keep track of every FloodID, you can change this behaviour by setting the unlimited_buffer config in your Cargo.toml:
```toml
[dependencies]
ap2024_unitn_cppenjoyers_drone = { git = "https://github.com/Cpp-enjoyers/drone.git", features =  ["unlimited_buffer"] }
```
