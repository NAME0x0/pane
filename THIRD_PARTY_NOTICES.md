# Third-Party Notices

Pane directly uses the following third-party software. This file will expand as the rust-vmm migration links additional components.

## windows-sys 0.61.2

- Source: https://github.com/microsoft/windows-rs
- License: MIT OR Apache-2.0 (Pane distributes it under the MIT option)
- Use: generated Windows Hypervisor Platform ABI definitions and delayed DLL loading primitives

Copyright (c) Microsoft Corporation.

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## vm-memory 0.17.1

- Source: https://github.com/rust-vmm/vm-memory
- License: Apache-2.0 OR BSD-3-Clause (Pane distributes it under the BSD-3-Clause option)
- Use: page-backed guest-memory allocation, bounded guest-address access, and stable host addresses for WHP mappings

Copyright 2017 The Chromium OS Authors. All rights reserved.

Redistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met:

- Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer.
- Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer in the documentation and/or other materials provided with the distribution.
- Neither the name of Google Inc. nor the names of its contributors may be used to endorse or promote products derived from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

## linux-loader 0.13.2

- Source: https://github.com/rust-vmm/linux-loader
- License: Apache-2.0 AND BSD-3-Clause
- Use: bzImage validation/loading, generated Linux boot-parameter structures and serialization, and kernel command-line placement
- Local source: `third_party/linux-loader`
- Pane modification: the `vm-memory` dependency is pinned to 0.17.1 with default features disabled so the upstream loader builds on Windows without Unix-only `rawfd` APIs

The complete required license texts and original copyright notices are preserved in `third_party/linux-loader/LICENSE-APACHE`, `third_party/linux-loader/LICENSE-BSD-3-Clause`, and the upstream source headers.

## virtio-queue 0.17.0

- Source: https://github.com/rust-vmm/vm-virtio
- License: Apache-2.0 AND BSD-3-Clause
- Use: split virtqueue validation, descriptor-chain iteration, available-ring consumption, and used-ring publication
- Local source: `third_party/virtio-queue`
- Pane modification: the `vm-memory` dependency has default features disabled so the upstream queue builds on Windows without Unix-only `rawfd` APIs

The complete required license texts and original copyright notices are preserved in `third_party/virtio-queue/LICENSE-APACHE`, `third_party/virtio-queue/LICENSE-BSD-3-Clause`, and the upstream source headers.

## virtio-bindings 0.2.7

- Source: https://github.com/rust-vmm/vm-virtio
- License: BSD-3-Clause OR Apache-2.0 (Pane distributes it under the BSD-3-Clause option)
- Use: spec-tracked virtio-MMIO register offsets, virtio-blk request/status/feature constants, ring descriptor flags, and device IDs

Copyright 2017 The Chromium OS Authors. All rights reserved.

Redistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met:

- Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer.
- Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer in the documentation and/or other materials provided with the distribution.
- Neither the name of Google Inc. nor the names of its contributors may be used to endorse or promote products derived from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
