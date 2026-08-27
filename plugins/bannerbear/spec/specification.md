# Bannerbear

| | |
|---|---|
| **API** | `https://api.bannerbear.com/v5` |
| **Idempotency** | No idempotency |
| **Reviewed by** | Claude Opus, 2026-08-26 |

**Notes**

| | |
|---|---|
| **OpenAPI** | `https://api.bannerbear.com/v5/openapi.json` |
| **Self-hosted** | no — SaaS only, no §5 |

## 1. Address

```
bannerbear://[{instance}]        # omitted instance = "default"
```

## 2. Configuration

```toml
[bannerbear.{instance}]          # [bannerbear] = [bannerbear.default]
```

| key | type | default | example |
|---|---|---|---|
| `api_key` | `String` | | `bb_…` |
| `poll` | `Duration` | `2s` | `2s` |
| `poll_image` | `Option<Duration>` | = `poll` | `2s` |
| `poll_animation` | `Option<Duration>` | = `poll` | `10s` |

## 3. Authentication & Authorization

| | |
|---|---|
| **Documentation** | [Authentication](https://developers.bannerbear.com/v5/#authentication) |
| **Probe** | `GET /v5/account` → `200` |

```
Authorization: Bearer {api_key}
```

## 4. Operations

### 4.1 image.create

| | |
|---|---|
| **Documentation** | [Create an image](https://developers.bannerbear.com/v5/#post-v5-images) |

```json
{ "func": "image.create", "args": { ... } }
```

### 4.1.1 Promise Param Schema

```json
{
  "description": "Render an image from a template with layer modifications. Seconds-scale. Resolves with the generated file URLs; rejects invalid_request on synchronous validation failure (unknown template → 404, invalid modification → 422) or render_failed if the render itself fails.",
  "type": "object",
  "properties": {
    "template": {
      "type": "string",
      "description": "Image template uid. Valid layer names are template-specific — see template.get."
    },
    "modifications": {
      "type": "object",
      "description": "Template-level and per-layer overrides.",
      "properties": {
        "template": {
          "type": "object",
          "description": "Template-level overrides.",
          "properties": {
            "width": {
              "type": "integer",
              "description": "Override width in pixels."
            },
            "height": {
              "type": "integer",
              "description": "Override height in pixels."
            },
            "transparent": {
              "type": "boolean",
              "description": "Transparent background."
            }
          }
        },
        "objects": {
          "type": "array",
          "description": "Per-layer modifications; \"name\" (or \"id\") must match a layer in the template. Remaining properties are layer-type-specific (text, color, font-size, … — see the object-modifications docs).",
          "items": {
            "type": "object",
            "properties": {
              "name": {
                "type": "string",
                "description": "Layer name (or use \"id\")."
              },
              "text": {
                "type": "string",
                "description": "For text layers."
              }
            },
            "required": [
              "name"
            ],
            "additionalProperties": true
          }
        }
      }
    },
    "formats": {
      "type": "array",
      "items": {
        "type": "string",
        "enum": [
          "jpg",
          "png",
          "pdf",
          "webp",
          "avif"
        ]
      },
      "description": "Output file formats."
    },
    "scale": {
      "type": "integer",
      "enum": [
        1,
        2,
        3,
        4
      ],
      "description": "Output scale multiplier."
    },
    "dpi": {
      "type": "integer",
      "description": "DPI metadata for print sizing (72–600)."
    },
    "quality": {
      "type": "integer",
      "description": "Compression quality for JPG/WebP (1–100)."
    },
    "proxy": {
      "type": "boolean",
      "description": "Proxy and resize external images before rendering."
    },
    "version": {
      "type": "integer",
      "description": "Target a specific template version."
    }
  },
  "required": [
    "template",
    "modifications"
  ]
}
```

### 4.1.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "properties": {
    "files": {
      "type": "object",
      "description": "= response.body.files (Bannerbear CDN URLs; copy out if the artifact must outlive Bannerbear's retention)"
    }
  }
}
```

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["invalid_request", "render_failed"]
    },
    "detail": {
      "description": "invalid_request: = response.body of the 4xx; render_failed: = response.body (the failed image object)"
    }
  },
  "required": ["code"]
}
```

### 4.1.3 Integration Request

```
POST /v5/images → 202
```

```json
{
  "type": "object",
  "properties": {
    "template": {
      "type": "string",
      "description": "= promise.param.template"
    },
    "modifications": {
      "type": "object",
      "description": "= promise.param.modifications"
    },
    "formats": {
      "type": "array",
      "description": "= promise.param.formats"
    },
    "scale": {
      "type": "integer",
      "description": "= promise.param.scale"
    },
    "dpi": {
      "type": "integer",
      "description": "= promise.param.dpi"
    },
    "quality": {
      "type": "integer",
      "description": "= promise.param.quality"
    },
    "proxy": {
      "type": "boolean",
      "description": "= promise.param.proxy"
    },
    "version": {
      "type": "integer",
      "description": "= promise.param.version"
    },
    "metadata": {
      "type": "string",
      "description": "= sanitize(promise.id)"
    }
  },
  "required": [
    "template",
    "modifications",
    "metadata"
  ]
}
```

### 4.1.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "uid": {
      "type": "string",
      "description": "External identity."
    },
    "status": {
      "type": "string",
      "enum": [
        "pending",
        "completed",
        "failed"
      ]
    },
    "template": {
      "type": "string"
    },
    "files": {
      "type": "object",
      "description": "Generated file URLs keyed by format (png, pdf, …). null while pending."
    },
    "error": {
      "type": "string",
      "description": "Only present when the render failed."
    },
    "metadata": {
      "type": "string",
      "description": "Echo of sanitize(promise.id)."
    },
    "self": {
      "type": "string",
      "description": "API URL of this image — the poll target."
    },
    "created_at": {
      "type": "string",
      "description": "ISO 8601"
    },
    "completed_at": {
      "type": "string",
      "description": "ISO 8601; null while pending."
    }
  }
}
```

### 4.1.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_poll` |

```python
import time
from urllib.parse import quote

import requests

API = "https://api.bannerbear.com/v5"


def _check(r):
    # 401 key invalid, 402 quota exhausted, 403 key lacks access —
    # an operator must act.
    if r.status_code in (401, 402, 403):
        raise Exception("halt", r.text)
    if r.status_code >= 400:
        raise Exception("release", r.text)


def image_create(cfg, promise):
    auth = {"Authorization": f"Bearer {cfg.api_key}"}
    args = promise.param["args"]

    # Unkeyed POST: a re-entry renders again — a duplicate render is
    # benign, costing one render credit.
    r = requests.post(
        f"{API}/images",
        headers=auth,
        json=args | {"metadata": sanitize(promise.id)},
        timeout=10,
    )
    if r.status_code in (400, 404, 422):
        return ("rejected", {"code": "invalid_request", "detail": r.json()})
    _check(r)
    img = r.json()

    failures = 0
    while img["status"] == "pending":
        if time.time() * 1000 >= promise.timeout_at:
            raise Exception("release", "promise timed out")
        time.sleep(cfg.poll_image.total_seconds())
        try:
            r = requests.get(f"{API}/images/{img['uid']}", headers=auth, timeout=10)
            _check(r)
        except Exception as e:
            if e.args[:1] == ("halt",):
                raise
            # The uid is unrecoverable on re-entry — absorb, bounded.
            failures += 1
            if failures >= 5:
                raise
            continue
        failures = 0
        img = r.json()

    if img["status"] == "completed":
        return ("resolved", {"files": img["files"]})
    return ("rejected", {"code": "render_failed", "detail": img})
```

### 4.2 animation.create

| | |
|---|---|
| **Documentation** | [Create an animation](https://developers.bannerbear.com/v5/#post-v5-animations) |

```json
{ "func": "animation.create", "args": { ... } }
```

### 4.2.1 Promise Param Schema

```json
{
  "description": "Render a motion graphic from an animation template. Resolves with the generated file URLs (mp4, or mov when transparent); rejects invalid_request on synchronous validation failure (unknown template → 404, invalid modification → 422) or render_failed if the render itself fails.",
  "type": "object",
  "properties": {
    "template": {
      "type": "string",
      "description": "Animation template uid (distinct from image templates)."
    },
    "modifications": {
      "type": "object",
      "description": "Template-level and per-layer overrides.",
      "properties": {
        "template": {
          "type": "object",
          "properties": {
            "width": {
              "type": "integer"
            },
            "height": {
              "type": "integer"
            },
            "fps": {
              "type": "integer",
              "enum": [24, 30, 60],
              "description": "Override output frame rate."
            },
            "transparent": {
              "type": "boolean",
              "description": "Transparent background; output is always MOV."
            }
          }
        },
        "objects": {
          "type": "array",
          "description": "Per-layer modifications, same properties as image.create's objects.",
          "items": {
            "type": "object",
            "properties": {
              "name": {
                "type": "string"
              }
            },
            "required": [
              "name"
            ],
            "additionalProperties": true
          }
        }
      }
    },
    "formats": {
      "type": "array",
      "items": {
        "type": "string",
        "enum": [
          "mp4",
          "mov"
        ]
      },
      "description": "Ignored when transparent is set — that always yields MOV."
    }
  },
  "required": [
    "template",
    "modifications"
  ]
}
```

### 4.2.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "properties": {
    "files": {
      "type": "object",
      "description": "= response.body.files"
    }
  }
}
```

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["invalid_request", "render_failed"]
    },
    "detail": {
      "description": "invalid_request: = response.body of the 4xx; render_failed: = response.body.error"
    }
  },
  "required": ["code"]
}
```

### 4.2.3 Integration Request

```
POST /v5/animations → 202
```

```json
{
  "type": "object",
  "properties": {
    "template": {
      "type": "string",
      "description": "= promise.param.template"
    },
    "modifications": {
      "type": "object",
      "description": "= promise.param.modifications"
    },
    "formats": {
      "type": "array",
      "description": "= promise.param.formats"
    },
    "metadata": {
      "type": "string",
      "description": "= sanitize(promise.id)"
    }
  },
  "required": [
    "template",
    "modifications",
    "metadata"
  ]
}
```

### 4.2.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "uid": {
      "type": "string",
      "description": "External identity."
    },
    "status": {
      "type": "string",
      "enum": ["queued", "rendering", "completed", "failed"]
    },
    "template": {
      "type": "string"
    },
    "files": {
      "type": "object",
      "description": "Output URLs keyed by format (mp4, or mov when transparent). Populated once completed."
    },
    "progress": {
      "type": "integer",
      "description": "Render progress 0–100."
    },
    "metadata": {
      "type": "string",
      "description": "Echo of sanitize(promise.id)."
    },
    "error": {
      "type": "string",
      "description": "Only present when the render failed."
    },
    "self": {
      "type": "string"
    },
    "created_at": {
      "type": "string",
      "description": "ISO 8601"
    },
    "completed_at": {
      "type": "string",
      "description": "ISO 8601; null until finished."
    }
  }
}
```

### 4.2.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_poll` |

```python
def animation_create(cfg, promise):
    auth = {"Authorization": f"Bearer {cfg.api_key}"}
    args = promise.param["args"]

    # Unkeyed POST, as in 4.1: re-entry renders again — benign.
    r = requests.post(
        f"{API}/animations",
        headers=auth,
        json=args | {"metadata": sanitize(promise.id)},
        timeout=10,
    )
    if r.status_code in (400, 404, 422):
        return ("rejected", {"code": "invalid_request", "detail": r.json()})
    _check(r)
    anim = r.json()

    failures = 0
    while anim["status"] in ("queued", "rendering"):
        if time.time() * 1000 >= promise.timeout_at:
            raise Exception("release", "promise timed out")
        time.sleep(cfg.poll_animation.total_seconds())
        try:
            r = requests.get(f"{API}/animations/{anim['uid']}", headers=auth, timeout=10)
            _check(r)
        except Exception as e:
            if e.args[:1] == ("halt",):
                raise
            failures += 1
            if failures >= 5:
                raise
            continue
        failures = 0
        anim = r.json()

    if anim["status"] == "completed":
        return ("resolved", {"files": anim["files"]})
    return ("rejected", {"code": "render_failed", "detail": anim["error"]})
```

### 4.3 template.get

| | |
|---|---|
| **Documentation** | [Retrieve an image template](https://developers.bannerbear.com/v5/#get-v5-image_templates-uid) |

```json
{ "func": "template.get", "args": { ... } }
```

### 4.3.1 Promise Param Schema

```json
{
  "description": "Fetch one image template, including its full config — every layer with every attribute; the layer names are the valid modification targets for image.create against this template.",
  "type": "object",
  "properties": {
    "uid": {
      "type": "string",
      "description": "Template uid."
    }
  },
  "required": [
    "uid"
  ]
}
```

### 4.3.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "description": "= response.body"
}
```

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["not_found"]
    }
  },
  "required": ["code"]
}
```

### 4.3.3 Integration Request

```
GET /v5/image_templates/{promise.param.uid} → 200
```

### 4.3.4 Integration Response

```json
{
  "type": "object",
  "description": "The image template: uid, name, and the full config — every layer with every attribute.",
  "properties": {
    "uid": {
      "type": "string"
    },
    "name": {
      "type": "string"
    }
  },
  "additionalProperties": true
}
```

### 4.3.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def template_get(cfg, promise):
    auth = {"Authorization": f"Bearer {cfg.api_key}"}
    uid = quote(promise.param["args"]["uid"], safe="")

    r = requests.get(f"{API}/image_templates/{uid}", headers=auth, timeout=10)
    if r.status_code == 404:
        return ("rejected", {"code": "not_found"})
    _check(r)
    return ("resolved", r.json())
```

### 4.4 template.list

| | |
|---|---|
| **Documentation** | [List image templates](https://developers.bannerbear.com/v5/#get-v5-image_templates) |

```json
{ "func": "template.list", "args": { ... } }
```

### 4.4.1 Promise Param Schema

```json
{
  "description": "List image templates in the workspace, for discovering template uids and composing image.create calls.",
  "type": "object",
  "properties": {
    "page": {
      "type": "integer",
      "description": "1-based page; 25 items per page; an empty array marks the end."
    }
  }
}
```

### 4.4.2 Promise Value Schema

#### Resolved

```json
{
  "type": "array",
  "description": "= response.body"
}
```

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["invalid_request"]
    },
    "detail": {
      "description": "= response.body of the 4xx"
    }
  },
  "required": ["code"]
}
```

### 4.4.3 Integration Request

```
GET /v5/image_templates?page={promise.param.page} → 200
```

### 4.4.4 Integration Response

```json
{
  "type": "array",
  "items": {
    "description": "Same as 4.3.4"
  }
}
```

### 4.4.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def template_list(cfg, promise):
    auth = {"Authorization": f"Bearer {cfg.api_key}"}
    args = promise.param["args"]

    # Pagination is the caller's loop (one promise per page), not ours.
    r = requests.get(f"{API}/image_templates", headers=auth, params=args, timeout=10)
    if r.status_code in (400, 422):
        return ("rejected", {"code": "invalid_request", "detail": r.json()})
    _check(r)
    return ("resolved", r.json())
```

