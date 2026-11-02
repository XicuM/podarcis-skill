# Third-party provenance

`scripts/openalex_cli.py` is adapted from Google DeepMind's science-skills repository:

- Copyright 2026 Google LLC.
- Upstream: https://github.com/google-deepmind/science-skills/blob/4de6786171f1479026da5d3a00f110703c8ba35e/skills/literature_search_openalex/scripts/openalex_cli.py
- Revision: `4de6786171f1479026da5d3a00f110703c8ba35e`, retrieved 2026-10-27.
- Upstream license: Apache-2.0, included in [APACHE-2.0.txt](APACHE-2.0.txt).

Podarcis modifications remove automatic loading of the home `.env`, allow an installed CLI entry point, and use bounded, atomic HTTPS PDF downloads that never replace a destination. Downloads try OpenAlex-hosted content when authenticated and then recorded PDF locations, including primary and best-OA locations, without changing scholarly providers. Upstream entity discovery, filtering, and retry behavior are retained.

The Podarcis-specific skill and vault command are licensed under AGPL-3.0; see [LICENSE.txt](LICENSE.txt). Earlier Podarcis code was distributed under the following MIT notice, retained for the reused ingestion and citation-status logic:

MIT License

Copyright (c) 2026 xicu

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
