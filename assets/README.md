# assets

One icon per implemented plugin, named after the plugin scheme:
`assets/<scheme>.png`.

Format: 128×128 PNG, RGBA, transparent background. The mark is trimmed to
its alpha bounding box and re-centred with 8% padding, so icons of very
different native aspect ratios carry the same optical weight when the
catalog renders them side by side at 20px.

Every icon is legible on a light page and on a dark one, so the catalog
needs a single file per plugin rather than a `-lb`/`-db` pair. Where a
provider ships only a monochrome silhouette, the fill is the brand colour
that survives both — Zendesk, whose primary `#03363D` disappears on a
dark page, is drawn in its secondary green `#78A300`.

Add an icon with `make-icon.py`, which applies the format and reports
whether the result holds up on both backgrounds:

```sh
pip install pillow cairosvg
python3 assets/make-icon.py <scheme> <source.svg|source.png> [#hex]
```

## Sources

| icon | source |
|---|---|
| `airflow.png` | [apache/airflow](https://github.com/apache/airflow) `airflow-core/docs/img/logos/airflow_transparent.png` |
| `bannerbear.png` | [n8n-io/n8n](https://github.com/n8n-io/n8n) `packages/nodes-base/nodes/Bannerbear/bannerbear.png` |
| `baserow.png` | [n8n-io/n8n](https://github.com/n8n-io/n8n) `packages/nodes-base/nodes/Baserow/baserow.svg` |
| `gotify.png` | [gotify/logo](https://github.com/gotify/logo) `gotify-logo.png` |
| `n8n.png` | [n8n-io/n8n](https://github.com/n8n-io/n8n) `packages/nodes-base/nodes/N8n/n8n.svg` |
| `rundeck.png` | [simple-icons](https://github.com/simple-icons/simple-icons) `icons/rundeck.svg`, filled `#F73F39` |
| `zendesk.png` | [simple-icons](https://github.com/simple-icons/simple-icons) `icons/zendesk.svg`, filled `#78A300` |

Each mark is the trademark of its owner and appears here only to identify
the provider the plugin integrates with.
