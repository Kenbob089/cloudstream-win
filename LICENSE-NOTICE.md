# License notice

This project is an **independent implementation** inspired by the open-source
[CloudStream](https://github.com/recloudstream/cloudstream) Android app, which is licensed under GPL-3.0.
No CloudStream source code is copied here. If you incorporate any GPL-3.0 code, the combined work
must be distributed under GPL-3.0.

Provider repositories (for example the Phisher repository) are third-party. Their plugin files are
Android `.cs3` packages that this app **never downloads or executes**; only their JSON metadata is read.
Their licenses and terms are their authors' own.

Dependencies: Rust crates (serde, serde_json, ureq, url, percent-encoding; MIT/Apache-2.0),
Windows App SDK (Microsoft, see its license). The built-in Internet Archive provider uses
archive.org's public APIs; respect their terms of use.

You are responsible for ensuring you have the right to access any content you play.
