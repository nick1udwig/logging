# `logging:nick.hypr`

dev tool: aggregate logs from users of your beta p2p app

## Usage

This package is for use with the [Hyperware `process_lib`](https://github.com/hyperware-ai/process_lib)s `logging` feature.
When you publish a beta application, use `hyperware_process_lib::logging::init_logging()` with the final argument pointing to your node you wish to aggregate the logs.
This log-aggregating node must be live on the network in order to receive the logs from your users.
Users' nodes will send logs of the level you have set to your log-aggregating node.
Your log-aggregating node will store the logs at `node_home/vfs/logging:nick.hypr/remote-log/your-package:your-publisher.os/your-process.log`.

## Getting `logging`

Install `logging:nick.hypr` from the Kinode App Store.
Alternatively, you can build from source and install yourself.

### Building & installing from source

```bash
git clone https://github.com/nick1udwig/logging
cd logging
kit b
kit s
```
