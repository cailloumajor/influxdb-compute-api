# Changelog

## [2.4.2](https://github.com/cailloumajor/influxdb-compute-api/compare/v2.4.1...v2.4.2) (2023-10-09)


### Bug Fixes

* **deps:** update rust crate reqwest to 0.11.22 ([a0f6d4f](https://github.com/cailloumajor/influxdb-compute-api/commit/a0f6d4fdcbdcbacfa54f1542af3b3da7b3cb8cb2))
* **deps:** update rust crate tokio to 1.33.0 ([5d7d1f3](https://github.com/cailloumajor/influxdb-compute-api/commit/5d7d1f309890603b9b4a3537aa6b5889783e2cb2))
* **deps:** update rust docker tag to v1.73.0 ([ef0ccba](https://github.com/cailloumajor/influxdb-compute-api/commit/ef0ccba28f64a01ce90a7bbef89189951c822fc2))
* use new (Rust 1.73.0) methods of LocalKey ([191fd44](https://github.com/cailloumajor/influxdb-compute-api/commit/191fd44f80fde226e9866a8e3fcf8cace7935447))

## [2.4.1](https://github.com/cailloumajor/influxdb-compute-api/compare/v2.4.0...v2.4.1) (2023-10-02)


### Bug Fixes

* cancel in-flight handling if request was cancelled ([6119cd0](https://github.com/cailloumajor/influxdb-compute-api/commit/6119cd011bf488e4541d91ae76c71cb39682f1c2))
* **deps:** update rust crate clap to 4.4.6 ([ce50963](https://github.com/cailloumajor/influxdb-compute-api/commit/ce5096356d201b5add7368f4cddcedf349a90de0))

## [2.4.0](https://github.com/cailloumajor/influxdb-compute-api/compare/v2.3.2...v2.4.0) (2023-09-22)


### Features

* implement current shift objective graph ([2dc8a8c](https://github.com/cailloumajor/influxdb-compute-api/commit/2dc8a8c5da13cd30262b23cf5030c356ccdf747c))
* implement current week objective graph ([6eaa28e](https://github.com/cailloumajor/influxdb-compute-api/commit/6eaa28ebe9a19608706563c9732db9b46f3a2ae4))


### Bug Fixes

* **deps:** update rust docker tag to v1.72.1 ([d0c14cf](https://github.com/cailloumajor/influxdb-compute-api/commit/d0c14cfe2f33b71b2940beac9be1d2852c5e97c3))

## [2.3.2](https://github.com/cailloumajor/influxdb-compute-api/compare/v2.3.1...v2.3.2) (2023-09-19)


### Bug Fixes

* **deps:** update rust crate clap to 4.4.4 ([963e69c](https://github.com/cailloumajor/influxdb-compute-api/commit/963e69cf097237d06a6e2072588b859f78fae8a3))
* remove use of `spawn_blocking` ([f281bfb](https://github.com/cailloumajor/influxdb-compute-api/commit/f281bfb57fb29517aa0b2e28228d45376a125362))

## [2.3.1](https://github.com/cailloumajor/influxdb-compute-api/compare/v2.3.0...v2.3.1) (2023-09-18)


### Bug Fixes

* allow empty part reference in performance data ([5a3e9a8](https://github.com/cailloumajor/influxdb-compute-api/commit/5a3e9a82342b12377fbb22e82101a6c344b49810))
* **deps:** update rust crate chrono to 0.4.31 ([b2edda0](https://github.com/cailloumajor/influxdb-compute-api/commit/b2edda05e86e3a24cf703713bdd6802978639aa0))

## [2.3.0](https://github.com/cailloumajor/influxdb-compute-api/compare/v2.2.0...v2.3.0) (2023-09-15)


### Features

* get common configuration from config API ([49dd4f4](https://github.com/cailloumajor/influxdb-compute-api/commit/49dd4f44c4344d554c93e7e1eace14d334f71fbf))
* switch to client timezone header ([22c3603](https://github.com/cailloumajor/influxdb-compute-api/commit/22c360302310f30a57a72bf34552e83c69a9c625))


### Bug Fixes

* **deps:** update rust crate bytes to 1.5.0 ([6b9b2d9](https://github.com/cailloumajor/influxdb-compute-api/commit/6b9b2d90e6af384c65b290c2dffcc00231f4c4a7))
* **deps:** update rust crate chrono to 0.4.30 ([bc19955](https://github.com/cailloumajor/influxdb-compute-api/commit/bc19955696d8d975039b2025d48994451e3760f7))
* **deps:** update rust crate clap to 4.4.3 ([d79c36c](https://github.com/cailloumajor/influxdb-compute-api/commit/d79c36cbd7b001f39aaa4092230f9cc1a02205ea))
* factorize roundtrip channel ([a7e1435](https://github.com/cailloumajor/influxdb-compute-api/commit/a7e14354488cc4a02938188ef6245f2eda56b7e6))
* use buffered channels ([22b891f](https://github.com/cailloumajor/influxdb-compute-api/commit/22b891f16e321469f45d651a415d6259370a62be))

## [2.2.0](https://github.com/cailloumajor/influxdb-compute-api/compare/v2.1.0...v2.2.0) (2023-09-06)


### Features

* use config API ([3ed0719](https://github.com/cailloumajor/influxdb-compute-api/commit/3ed0719c1146c596678732947d9e0ad08c1653c2))


### Bug Fixes

* **deps:** update rust crate chrono to 0.4.29 ([8a293d5](https://github.com/cailloumajor/influxdb-compute-api/commit/8a293d5651fbdec621ccef2ab2bab8f79b16f8b2))

## [2.1.0](https://github.com/cailloumajor/influxdb-compute-api/compare/v2.0.0...v2.1.0) (2023-09-01)


### Features

* use target cycle time to calculate performance ([3d28619](https://github.com/cailloumajor/influxdb-compute-api/commit/3d286190b6db1a109c4373bbd23332f0f2bef3c1))

## [2.0.0](https://github.com/cailloumajor/influxdb-compute-api/compare/v1.0.0...v2.0.0) (2023-09-01)


### ⚠ BREAKING CHANGES

* get client time as a query parameter
* implement target cycle time timeline query param

### Features

* get client time as a query parameter ([faad666](https://github.com/cailloumajor/influxdb-compute-api/commit/faad666b5e69a605dcc51dfab409142786106122))
* implement target cycle time timeline query param ([6487b7c](https://github.com/cailloumajor/influxdb-compute-api/commit/6487b7c4adb8c2fa91ecb4c8ce3cd4d85f2ef9f0))


### Bug Fixes

* **deps:** update rust crate chrono to 0.4.28 ([6212aaf](https://github.com/cailloumajor/influxdb-compute-api/commit/6212aafa37b610939378654156524e14810b011f))
* **deps:** update rust crate clap to 4.4.1 ([aa46e49](https://github.com/cailloumajor/influxdb-compute-api/commit/aa46e494dc5e8f4673b03c79ece95091b2971d39))
* **deps:** update rust crate clap to 4.4.2 ([16192c0](https://github.com/cailloumajor/influxdb-compute-api/commit/16192c00f4096c493242c2671d6fca3114641294))
* **deps:** update rust crate url to 2.4.1 ([38969d5](https://github.com/cailloumajor/influxdb-compute-api/commit/38969d56a65eed4ab15a48810cb74aae39e4cb50))

## [1.0.0](https://github.com/cailloumajor/influxdb-compute-api/compare/v0.1.7...v1.0.0) (2023-08-27)


### Features

* implement performance ratio ([8ff3ca3](https://github.com/cailloumajor/influxdb-compute-api/commit/8ff3ca37931fb5ef0df54e812a8d79391ee98950))


### Bug Fixes

* **deps:** update rust crate serde to 1.0.188 ([6bddb8d](https://github.com/cailloumajor/influxdb-compute-api/commit/6bddb8d820703274087a2be3cb85d69559c0ec42))

## [0.1.7](https://github.com/cailloumajor/influxdb-compute-api/compare/v0.1.6...v0.1.7) (2023-08-25)


### Bug Fixes

* **deps:** update rust crate clap to 4.4.0 ([994fed5](https://github.com/cailloumajor/influxdb-compute-api/commit/994fed50f3ebae54d551873f162c9a1eb6d896df))
* **deps:** update rust crate reqwest to 0.11.19 ([b15e71e](https://github.com/cailloumajor/influxdb-compute-api/commit/b15e71e596dae58f819ed7a23838e1d8f614f6fb))
* **deps:** update rust crate reqwest to 0.11.20 ([3e755fb](https://github.com/cailloumajor/influxdb-compute-api/commit/3e755fb43c36977fd8a0d41e011606e87216d9ff))
* **deps:** update rust crate serde to 1.0.186 ([c796424](https://github.com/cailloumajor/influxdb-compute-api/commit/c796424f03f8b0c76bd009ec6a261c4a0f9a726c))
* **deps:** update rust docker tag to v1.72.0 ([24d1bd4](https://github.com/cailloumajor/influxdb-compute-api/commit/24d1bd4dfc27a636f77e7865c48c8a664cd1673f))

## [0.1.6](https://github.com/cailloumajor/influxdb-compute-api/compare/v0.1.5...v0.1.6) (2023-08-21)


### Bug Fixes

* **deps:** update rust crate anyhow to 1.0.75 ([c24c286](https://github.com/cailloumajor/influxdb-compute-api/commit/c24c28643e504253003f96cbbf5a42b6775e3948))
* **deps:** update rust crate clap to 4.3.22 ([a9698a5](https://github.com/cailloumajor/influxdb-compute-api/commit/a9698a5b14ea12cf9d1653bcea27dff72e4f84fb))
* **deps:** update rust crate clap to 4.3.23 ([8c4b882](https://github.com/cailloumajor/influxdb-compute-api/commit/8c4b882b79325f9e3e8d8f56cea829799ae51cd6))
* **deps:** update rust crate serde to 1.0.185 ([50dae23](https://github.com/cailloumajor/influxdb-compute-api/commit/50dae2336c7693c05c01d196cf7700c791f544ca))
* **deps:** update rust crate tokio to 1.32.0 ([aaaef48](https://github.com/cailloumajor/influxdb-compute-api/commit/aaaef484bb21013e5119ae40e0e0eb53079b4a07))

## [0.1.5](https://github.com/cailloumajor/influxdb-compute-api/compare/v0.1.4...v0.1.5) (2023-08-15)


### Bug Fixes

* **deps:** update rust crate anyhow to 1.0.74 ([91d0433](https://github.com/cailloumajor/influxdb-compute-api/commit/91d04332bccd268016abe4ed36056e246d874603))

## [0.1.4](https://github.com/cailloumajor/influxdb-compute-api/compare/v0.1.3...v0.1.4) (2023-08-14)


### Bug Fixes

* **deps:** update rust crate axum to 0.6.20 ([c76a38f](https://github.com/cailloumajor/influxdb-compute-api/commit/c76a38f5320609cca910f5808be3356101d642b9))
* **deps:** update rust crate clap to 4.3.21 ([6107756](https://github.com/cailloumajor/influxdb-compute-api/commit/6107756a924872dc05f76f21c529e381c9098979))
* **deps:** update rust crate serde to 1.0.183 ([eae7b8e](https://github.com/cailloumajor/influxdb-compute-api/commit/eae7b8eb7ce95f85b6fd0b940c256f79fc2ace02))
* **deps:** update rust crate tokio to 1.31.0 ([f4d70da](https://github.com/cailloumajor/influxdb-compute-api/commit/f4d70da418e2b3466a3d6f8f65c7a729996adfa7))
* **deps:** update rust docker tag to v1.71.1 ([eec2fbb](https://github.com/cailloumajor/influxdb-compute-api/commit/eec2fbbda4e57fec394a8c3ac53dcedb1dc67083))

## [0.1.3](https://github.com/cailloumajor/influxdb-compute-api/compare/v0.1.2...v0.1.3) (2023-07-25)


### Bug Fixes

* **deps:** update rust crate clap to 4.3.15 ([0f93b61](https://github.com/cailloumajor/influxdb-compute-api/commit/0f93b614345fd09795dc5b9a0e00fcc058f2cd1c))
* **deps:** update rust crate clap to 4.3.17 ([1113173](https://github.com/cailloumajor/influxdb-compute-api/commit/1113173386502709c8f3aa16a8ddd2927187d707))
* **deps:** update rust crate clap to 4.3.19 ([f9f9e33](https://github.com/cailloumajor/influxdb-compute-api/commit/f9f9e33bf7959c2db7cdc39705c653661e67c430))
* **deps:** update rust crate rmp-serde to 1.1.2 ([eb22a42](https://github.com/cailloumajor/influxdb-compute-api/commit/eb22a42dbd328ee091e7671d0cc56cbb3a229172))
* **deps:** update rust crate serde to 1.0.173 ([6db5f1f](https://github.com/cailloumajor/influxdb-compute-api/commit/6db5f1f57fbd2d06794e858eb58a7f57588ed393))
* **deps:** update rust crate serde to 1.0.174 ([df23328](https://github.com/cailloumajor/influxdb-compute-api/commit/df23328e27b34925cdd227e1cad624ece52361c2))
* **deps:** update rust crate serde to 1.0.175 ([c8d5314](https://github.com/cailloumajor/influxdb-compute-api/commit/c8d5314f2b70097b264abae61ef7b5f2fef0f7ff))
* **deps:** update rust crate signal-hook to 0.3.17 ([1a8359c](https://github.com/cailloumajor/influxdb-compute-api/commit/1a8359c7231c29750288c3fb16eccf98ba602936))

## [0.1.2](https://github.com/cailloumajor/influxdb-compute-api/compare/v0.1.1...v0.1.2) (2023-07-17)


### Bug Fixes

* **deps:** update rust crate anyhow to 1.0.72 ([42296de](https://github.com/cailloumajor/influxdb-compute-api/commit/42296de7f83f4abc352a8bc2b9d180f4df54b184))
* **deps:** update rust crate axum to 0.6.19 ([d597bb5](https://github.com/cailloumajor/influxdb-compute-api/commit/d597bb5c6dea6bb01bec5c0a88e3562d59d9e5a8))
* **deps:** update rust crate clap to 4.3.12 ([f755d38](https://github.com/cailloumajor/influxdb-compute-api/commit/f755d38b9753713f7e4d8c59e88d250c7c136599))
* **deps:** update rust crate serde to 1.0.169 ([ea22865](https://github.com/cailloumajor/influxdb-compute-api/commit/ea22865f6618c06a498dd206fb1dbe03383908b1))
* **deps:** update rust crate serde to 1.0.171 ([4c2c468](https://github.com/cailloumajor/influxdb-compute-api/commit/4c2c4686d508fd6a7a36c98ba4740f4e66d2c01f))
* **deps:** update rust crate signal-hook to 0.3.16 ([3762dc2](https://github.com/cailloumajor/influxdb-compute-api/commit/3762dc23b0b98a93f43058014438a03b9fa90446))
* **deps:** update rust docker tag to v1.71.0 ([e43f562](https://github.com/cailloumajor/influxdb-compute-api/commit/e43f5622b0dd17ddbb26810c99568845f27c651e))

## [0.1.1](https://github.com/cailloumajor/influxdb-compute-api/compare/v0.1.0...v0.1.1) (2023-07-06)


### Bug Fixes

* **ci:** add missing actions permissions ([b94a304](https://github.com/cailloumajor/influxdb-compute-api/commit/b94a304f0876538e5f32a416ada348f3d105dad9))

## 0.1.0 (2023-07-06)


### Features

* add Docker stuff ([4975bd4](https://github.com/cailloumajor/influxdb-compute-api/commit/4975bd4e67c6f62524b64fa982d990bc82eabfe5))
* implement healthcheck binary ([9b9f3a3](https://github.com/cailloumajor/influxdb-compute-api/commit/9b9f3a37db4ccd5dc1de1fe7cf17c9eec019ca46))
* implement HTTP API ([aff47b0](https://github.com/cailloumajor/influxdb-compute-api/commit/aff47b095078528c72f9aa3520b21337bec6cf65))
* implement InfluxDB query ([c0c4594](https://github.com/cailloumajor/influxdb-compute-api/commit/c0c4594d0d63d443c82bd4dfb78215512447d766))
* implement timeline InfluxDB handler ([8e140a2](https://github.com/cailloumajor/influxdb-compute-api/commit/8e140a2088986c77bd8007b524cb944480c68eb7))
* use abstract color index ([3fb3f59](https://github.com/cailloumajor/influxdb-compute-api/commit/3fb3f599b738b7b23f5e7fe49e2fa557791b9992))
