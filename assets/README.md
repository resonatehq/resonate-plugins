# assets

One icon per catalog entry, named after the provider: `assets/<slug>.png`,
where the slug is the provider's display name lowercased with every
non-alphanumeric character removed (`AWS Glue` → `awsglue.png`).

Format: 128×128 PNG, RGBA, transparent background. The mark is trimmed to
its alpha bounding box and re-centred with 8% padding, so icons of very
different native aspect ratios carry the same optical weight when the
catalog renders them at 20px.

## Sources

| count | source |
|---|---|
| 340 | the provider's own mark, from [n8n](https://github.com/n8n-io/n8n)'s `packages/nodes-base/nodes/**` node icons and [simple-icons](https://github.com/simple-icons/simple-icons) |
| 104 | a generated monogram — the provider's initials on a colour derived from its name, for providers neither source carries |

A mark that reads on a light page but disappears on a dark one — or the
reverse — sits on a rounded plate whose colour is the opposite of the
mark's own luminance. A dark mark gets a light plate and a light mark gets
a dark one, so the mark stays legible on both while the plate blends into
whichever background matches it. 91 of the 444 needed this.

`make-icon.py` applies the format to a single new icon and reports whether
the result holds up on both backgrounds:

```sh
pip install pillow cairosvg
python3 assets/make-icon.py <slug> <source.svg|source.png> [#hex]
```

Each mark is the trademark of its owner and appears here only to identify
the provider the plugin integrates with.
