# Third-Party Licenses

RetroAmp is built on top of many excellent open-source projects. This file
attributes them and reproduces the license notices that require it.

For exact version numbers, see [`src-tauri/Cargo.toml`](src-tauri/Cargo.toml),
[`src-tauri/Cargo.lock`](src-tauri/Cargo.lock), and
[`package.json`](package.json).

---

## Direct dependencies — Rust crates

| Crate                  | License                |
| ---------------------- | ---------------------- |
| `tauri`                | Apache-2.0 OR MIT      |
| `tauri-plugin-shell`   | Apache-2.0 OR MIT      |
| `tauri-plugin-dialog`  | Apache-2.0 OR MIT      |
| `tauri-plugin-updater` | Apache-2.0 OR MIT      |
| `tauri-plugin-process` | Apache-2.0 OR MIT      |
| `symphonia`            | MPL-2.0                |
| `cpal`                 | Apache-2.0             |
| `rustfft`              | MIT OR Apache-2.0      |
| `rubato`               | MIT                    |
| `ringbuf`              | MIT OR Apache-2.0      |
| `audioadapter-buffers` | MIT OR Apache-2.0      |
| `audiopus`             | ISC                    |
| `fdk-aac` (Rust wrapper) | MIT                  |
| `fdk-aac-sys` + bundled libfdk-aac | see Fraunhofer notice below |
| `rusqlite`             | MIT                    |
| `lofty`                | MIT OR Apache-2.0      |
| `ureq`                 | MIT OR Apache-2.0      |
| `reqwest`              | MIT OR Apache-2.0      |
| `rusty_ytdl`           | MIT OR Apache-2.0      |
| `ytmapi-rs`            | MIT                    |
| `souvlaki`             | MIT                    |
| `librespot` (optional, `--features spotify`) | MIT      |
| `zip`                  | MIT                    |
| `serde`, `serde_json`  | MIT OR Apache-2.0      |
| `thiserror`            | MIT OR Apache-2.0      |
| `log`, `env_logger`    | MIT OR Apache-2.0      |
| `tokio`                | MIT                    |
| `chrono`               | MIT OR Apache-2.0      |
| `dirs`, `toml`         | MIT OR Apache-2.0      |
| `sha1`, `sha2`         | MIT OR Apache-2.0      |
| `base64`               | MIT OR Apache-2.0      |
| `percent-encoding`     | MIT OR Apache-2.0      |
| `rand`                 | MIT OR Apache-2.0      |

The full transitive dependency tree (and the full text of every license) can
be regenerated from a checked-out source tree with
[`cargo-about`](https://github.com/EmbarkStudios/cargo-about).

## Direct dependencies — JavaScript / npm

| Package                     | License    |
| --------------------------- | ---------- |
| `react`, `react-dom`        | MIT        |
| `@tauri-apps/api`           | Apache-2.0 OR MIT |
| `@tauri-apps/plugin-dialog`, `plugin-process`, `plugin-updater` | Apache-2.0 OR MIT |
| `butterchurn`               | MIT        |
| `butterchurn-presets`       | MIT        |
| `vite`, `@vitejs/plugin-react` | MIT     |
| `typescript`                | Apache-2.0 |
| `@types/react`, `@types/react-dom` | MIT |

## Code ported from other projects

Two source files contain code ported from
[Webamp](https://github.com/captbaritone/webamp) by Jordan Eldredge
(MIT License):

- `src/skin/sprites.ts` — sprite coordinate tables for Winamp 2.x bitmaps
- `src/skin/charmap.ts` — bitmap-font character map for `text.bmp`

Both files retain header comments noting the upstream source. Webamp's MIT
license is reproduced below:

```
MIT License

Copyright (c) 2017 Jordan Eldredge

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## Required notice: Fraunhofer FDK AAC Codec Library

RetroAmp uses [libfdk-aac](https://github.com/mstorsjo/fdk-aac) (vendored via
the `fdk-aac-sys` Rust crate) to decode HE-AAC and HE-AACv2 internet radio
streams. The Fraunhofer license requires that the following notice be
retained in the documentation and/or other materials accompanying binary
redistributions. It is reproduced below verbatim.

The complete source code of libfdk-aac as used by RetroAmp is available
from the `fdk-aac-sys` crate on crates.io and from the upstream repository
at <https://github.com/mstorsjo/fdk-aac>.

```
Software License for The Fraunhofer FDK AAC Codec Library for Android

© Copyright  1995 - 2018 Fraunhofer-Gesellschaft zur Förderung der angewandten
Forschung e.V. All rights reserved.

 1.    INTRODUCTION
The Fraunhofer FDK AAC Codec Library for Android ("FDK AAC Codec") is software
that implements the MPEG Advanced Audio Coding ("AAC") encoding and decoding
scheme for digital audio. This FDK AAC Codec software is intended to be used on
a wide variety of Android devices.

AAC's HE-AAC and HE-AAC v2 versions are regarded as today's most efficient
general perceptual audio codecs. AAC-ELD is considered the best-performing
full-bandwidth communications codec by independent studies and is widely
deployed. AAC has been standardized by ISO and IEC as part of the MPEG
specifications.

Patent licenses for necessary patent claims for the FDK AAC Codec (including
those of Fraunhofer) may be obtained through Via Licensing
(www.vialicensing.com) or through the respective patent owners individually for
the purpose of encoding or decoding bit streams in products that are compliant
with the ISO/IEC MPEG audio standards. Please note that most manufacturers of
Android devices already license these patent claims through Via Licensing or
directly from the patent owners, and therefore FDK AAC Codec software may
already be covered under those patent licenses when it is used for those
licensed purposes only.

Commercially-licensed AAC software libraries, including floating-point versions
with enhanced sound quality, are also available from Fraunhofer. Users are
encouraged to check the Fraunhofer website for additional applications
information and documentation.

2.    COPYRIGHT LICENSE

Redistribution and use in source and binary forms, with or without modification,
are permitted without payment of copyright license fees provided that you
satisfy the following conditions:

You must retain the complete text of this software license in redistributions of
the FDK AAC Codec or your modifications thereto in source code form.

You must retain the complete text of this software license in the documentation
and/or other materials provided with redistributions of the FDK AAC Codec or
your modifications thereto in binary form. You must make available free of
charge copies of the complete source code of the FDK AAC Codec and your
modifications thereto to recipients of copies in binary form.

The name of Fraunhofer may not be used to endorse or promote products derived
from this library without prior written permission.

You may not charge copyright license fees for anyone to use, copy or distribute
the FDK AAC Codec software or your modifications thereto.

Your modified versions of the FDK AAC Codec must carry prominent notices stating
that you changed the software and the date of any change. For modified versions
of the FDK AAC Codec, the term "Fraunhofer FDK AAC Codec Library for Android"
must be replaced by the term "Third-Party Modified Version of the Fraunhofer FDK
AAC Codec Library for Android."

3.    NO PATENT LICENSE

NO EXPRESS OR IMPLIED LICENSES TO ANY PATENT CLAIMS, including without
limitation the patents of Fraunhofer, ARE GRANTED BY THIS SOFTWARE LICENSE.
Fraunhofer provides no warranty of patent non-infringement with respect to this
software.

You may use this FDK AAC Codec software or modifications thereto only for
purposes that are authorized by appropriate patent licenses.

4.    DISCLAIMER

This FDK AAC Codec software is provided by Fraunhofer on behalf of the copyright
holders and contributors "AS IS" and WITHOUT ANY EXPRESS OR IMPLIED WARRANTIES,
including but not limited to the implied warranties of merchantability and
fitness for a particular purpose. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR
CONTRIBUTORS BE LIABLE for any direct, indirect, incidental, special, exemplary,
or consequential damages, including but not limited to procurement of substitute
goods or services; loss of use, data, or profits, or business interruption,
however caused and on any theory of liability, whether in contract, strict
liability, or tort (including negligence), arising in any way out of the use of
this software, even if advised of the possibility of such damage.

5.    CONTACT INFORMATION

Fraunhofer Institute for Integrated Circuits IIS
Attention: Audio and Multimedia Departments - FDK AAC LL
Am Wolfsmantel 33
91058 Erlangen, Germany

www.iis.fraunhofer.de/amm
amm-info@iis.fraunhofer.de
```

RetroAmp uses libfdk-aac unmodified, as vendored by the `fdk-aac-sys` crate.

---

## Bundled Winamp skins

The `skins/` directory contains a curated selection of classic `.wsz` skins
included as a starter pack. All rights remain with their original authors.
If you are the author of a bundled skin and would like it removed, please
open an issue or contact the maintainer.
