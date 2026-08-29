# Baserow

| | |
|---|---|
| **API** | `{base_url}/api` |
| **Idempotency** | No idempotency key. `table.import` stamps `sanitize(promise.id)` into the job's `original_file_name` — free text, `maxLength` 255, no charset constraint, echoed on the job and returned by `GET /api/jobs/?type=file_import` — and recovers the job by that stamp; jobs are retained `BASEROW_JOB_EXPIRATION_TIME_LIMIT` (30 days). No other endpoint accepts a client-supplied identity |
| **Reviewed by** | Claude Opus 5, 2026-08-29 |

**Notes**

| | |
|---|---|
| **OpenAPI** | `https://api.baserow.io/api/schema.json` |
| **Self-hosted** | yes — §5 |

## 1. Address

```
baserow://[{instance}]           # omitted instance = "default"
```

## 2. Configuration

```toml
[baserow.{instance}]             # [baserow] = [baserow.default]
```

| key | type | default | example |
|---|---|---|---|
| `base_url` | `String` | | `https://baserow.acme.com` |
| `email` | `String` | | `resonate@acme.com` |
| `password` | `String` | | `…` |
| `poll` | `Duration` | `2s` | `2s` |
| `poll_export` | `Option<Duration>` | `= poll` | `2s` |
| `poll_table` | `Option<Duration>` | `= poll` | `5s` |

## 3. Authentication & Authorization

| | |
|---|---|
| **Documentation** | [Authenticate a user](https://api.baserow.io/api/redoc/#tag/User/operation/token_auth) |
| **Probe** | `GET /api/applications/` → `200` |

```
POST /api/user/token-auth/  {"email": "{email}", "password": "{password}"}  → 200 {"access_token": …}
Authorization: JWT {access_token}
```

## 4. Operations

### 4.1 row.list

| | |
|---|---|
| **Documentation** | [List rows](https://api.baserow.io/api/redoc/#tag/Database-table-rows/operation/list_database_table_rows) |

```json
{ "func": "row.list", "args": { ... } }
```

### 4.1.1 Promise Param Schema

```json
{
  "description": "List one page of a table's rows. Resolves with the page. Rejects table_not_found, field_not_found or view_not_found when a named table, field or view does not exist, and invalid_request when a filter, sort, search mode or page size is not accepted (page size above 200, an order_by or filter naming a field the table does not have).",
  "type": "object",
  "properties": {
    "table_id": {
      "type": "integer",
      "description": "Table to read. Enumerate via database.list or table.list."
    },
    "page": {
      "type": "integer",
      "minimum": 1,
      "description": "1 when omitted."
    },
    "size": {
      "type": "integer",
      "minimum": 1,
      "maximum": 200,
      "description": "Rows per page. Capped by BASEROW_ROW_PAGE_SIZE_LIMIT, 200 by default; a larger value answers ERROR_PAGE_SIZE_LIMIT. 100 when omitted."
    },
    "search": {
      "type": "string",
      "description": "Only rows whose data matches this query are returned."
    },
    "search_mode": {
      "type": "string",
      "enum": ["full-text-with-count", "compat"],
      "description": "full-text-with-count uses Postgres full-text search; compat matches the term exactly, whitespace included. full-text-with-count when omitted."
    },
    "order_by": {
      "type": "array",
      "description": "Field ids, or field names when user_field_names is true, joined with commas on the wire. A leading \"-\" orders that field descending.",
      "items": {
        "type": "string"
      }
    },
    "include": {
      "type": "array",
      "description": "Restrict the response to these fields — field_{id} names, or field names when user_field_names is true. Joined with commas on the wire.",
      "items": {
        "type": "string"
      }
    },
    "exclude": {
      "type": "array",
      "description": "Drop these fields from the response. Same naming as include.",
      "items": {
        "type": "string"
      }
    },
    "user_field_names": {
      "type": "boolean",
      "description": "true makes the response keys the user-visible field names instead of field_{id}. false when omitted."
    },
    "view_id": {
      "type": "integer",
      "description": "Apply this view's saved filters and sorts. Enumerate via view.list."
    },
    "filters": {
      "type": "object",
      "description": "Filter tree, JSON-serialised into the query string. field is a field id, or a field name when user_field_names is true. Supersedes any filter__{field}__{type} form.",
      "properties": {
        "filter_type": {
          "type": "string",
          "enum": ["AND", "OR"]
        },
        "filters": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": true
          }
        }
      },
      "required": ["filter_type"],
      "additionalProperties": true
    }
  },
  "required": [
    "table_id"
  ],
  "additionalProperties": false
}
```

### 4.1.2 Promise Value Schema

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
      "enum": ["table_not_found", "field_not_found", "view_not_found", "invalid_request"]
    },
    "detail": {
      "description": "= response.body.detail of the 400/404"
    }
  },
  "required": ["code"]
}
```

### 4.1.3 Integration Request

```
GET /api/database/rows/table/{promise.param.table_id}/{?page,size,search,search_mode,order_by,include,exclude,user_field_names,view_id,filters = promise.param.*} → 200
```

### 4.1.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "count": {
      "type": "integer",
      "description": "Total rows matching the query, across all pages."
    },
    "next": {
      "type": ["string", "null"],
      "description": "URL of the next page; null on the last page."
    },
    "previous": {
      "type": ["string", "null"],
      "description": "URL of the previous page; null on the first page."
    },
    "results": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "id": {
            "type": "integer"
          },
          "order": {
            "type": "string",
            "description": "Decimal string; the row's position in the table."
          }
        },
        "required": ["id", "order"],
        "additionalProperties": true,
        "description": "One key per field: field_{id}, or the field's name when user_field_names is true. The value's shape depends on the field type."
      }
    },
    "error": {
      "type": "string",
      "description": "Present on 4xx only. Machine readable failure code."
    },
    "detail": {
      "description": "Present on 4xx only. String or object."
    }
  },
  "required": ["count", "next", "previous", "results"]
}
```

### 4.1.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
import json
import time
from urllib.parse import quote

import requests

EXPORT_TERMINAL = ("finished", "failed", "cancelled", "expired")
JOB_TERMINAL = ("finished", "failed", "cancelled")

# The `error` field of a Baserow 4xx body is a documented machine-readable
# enum, listed per endpoint in the OpenAPI. These two name a standing
# permission problem: the credentials are valid, but the account is not a
# member of the table's workspace, or holds no rights on the table.
PERMISSION_ERRORS = ("ERROR_USER_NOT_IN_GROUP", "ERROR_NO_PERMISSION_TO_TABLE")


def _error(r):
    try:
        return r.json().get("error")
    except ValueError:
        return None


def _detail(r):
    try:
        return r.json().get("detail")
    except ValueError:
        return r.text


def _check(r):
    # 401 ERROR_INVALID_ACCESS_TOKEN: the access token is missing, expired or
    # invalid. 402 ERROR_FEATURE_NOT_AVAILABLE: the instance holds no licence
    # for the requested feature (the json, xml, excel and file exporters).
    if r.status_code in (401, 402, 403):
        raise Exception("halt", r.text)
    if r.status_code >= 400 and _error(r) in PERMISSION_ERRORS:
        raise Exception("halt", r.text)
    if r.status_code == 429 or r.status_code >= 500:
        raise Exception("release", r.text)
    return r


def _token(cfg):
    r = requests.post(
        f"{cfg.base_url}/api/user/token-auth/",
        json={"email": cfg.email, "password": cfg.password},
        timeout=10,
    )
    if r.status_code == 429 or r.status_code >= 500:
        raise Exception("release", r.text)
    # 401 ERROR_INVALID_CREDENTIALS / ERROR_DEACTIVATED_USER /
    # ERROR_AUTH_PROVIDER_DISABLED / ERROR_EMAIL_VERIFICATION_REQUIRED.
    if r.status_code >= 400:
        raise Exception("halt", r.text)
    # BASEROW_ACCESS_TOKEN_LIFETIME_MINUTES is 10 by default, so a poll loop
    # outliving that window re-mints on the next 401.
    return {"Authorization": f"JWT {r.json()['access_token']}"}


def _query(args, keys):
    # A boolean reaches the query string as "true"/"false"; Python's
    # True/False is not a form Baserow documents. Field lists (order_by,
    # include, exclude) are comma-joined into a single value.
    params = {}
    for k in keys:
        if k not in args:
            continue
        v = args[k]
        if isinstance(v, bool):
            v = "true" if v else "false"
        elif isinstance(v, list):
            v = ",".join(str(x) for x in v)
        params[k] = v
    return params


def row_list(cfg, promise):
    auth = _token(cfg)
    args = promise.param["args"]
    table_id = quote(str(args["table_id"]), safe="")

    params = _query(
        args,
        ("page", "size", "search", "search_mode", "order_by", "include",
         "exclude", "user_field_names", "view_id"),
    )
    if "filters" in args:
        params["filters"] = json.dumps(args["filters"])

    r = requests.get(
        f"{cfg.base_url}/api/database/rows/table/{table_id}/",
        headers=auth,
        params=params,
        timeout=10,
    )
    if r.status_code == 404:
        e = _error(r)
        if e == "ERROR_TABLE_DOES_NOT_EXIST":
            return ("rejected", {"code": "table_not_found", "detail": _detail(r)})
        if e == "ERROR_FIELD_DOES_NOT_EXIST":
            return ("rejected", {"code": "field_not_found", "detail": _detail(r)})
        if e == "ERROR_VIEW_DOES_NOT_EXIST":
            return ("rejected", {"code": "view_not_found", "detail": _detail(r)})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": _detail(r)})
    return ("resolved", r.json())
```


### 4.2 row.get

| | |
|---|---|
| **Documentation** | [Get row](https://api.baserow.io/api/redoc/#tag/Database-table-rows/operation/get_database_table_row) |

```json
{ "func": "row.get", "args": { ... } }
```

### 4.2.1 Promise Param Schema

```json
{
  "description": "Read one row. Resolves with the row. Rejects table_not_found, row_not_found or view_not_found when the named table, row or view does not exist, and invalid_request on a malformed query.",
  "type": "object",
  "properties": {
    "table_id": {
      "type": "integer"
    },
    "row_id": {
      "type": "integer",
      "description": "Row to read. Enumerate via row.list."
    },
    "include": {
      "type": "array",
      "description": "Extra attributes to attach to the row. metadata adds the row's row_comments_notification_mode and related per-row data. Joined with commas on the wire.",
      "items": {
        "type": "string"
      }
    },
    "user_field_names": {
      "type": "boolean",
      "description": "true makes the response keys the user-visible field names instead of field_{id}. false when omitted."
    },
    "view": {
      "type": "integer",
      "description": "Read the row as seen through this view: the view's field permissions and visibility apply."
    }
  },
  "required": [
    "table_id",
    "row_id"
  ],
  "additionalProperties": false
}
```

### 4.2.2 Promise Value Schema

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
      "enum": ["table_not_found", "row_not_found", "view_not_found", "invalid_request"]
    },
    "detail": {
      "description": "= response.body.detail of the 400/404"
    }
  },
  "required": ["code"]
}
```

### 4.2.3 Integration Request

```
GET /api/database/rows/table/{promise.param.table_id}/{promise.param.row_id}/{?include,user_field_names,view = promise.param.*} → 200
```

### 4.2.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "integer"
    },
    "order": {
      "type": "string",
      "description": "Decimal string; the row's position in the table."
    },
    "error": {
      "type": "string",
      "description": "Present on 4xx only. Machine readable failure code."
    },
    "detail": {
      "description": "Present on 4xx only. String or object."
    }
  },
  "required": ["id", "order"],
  "additionalProperties": true,
  "description": "One key per field: field_{id}, or the field's name when user_field_names is true."
}
```

### 4.2.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def row_get(cfg, promise):
    auth = _token(cfg)
    args = promise.param["args"]
    table_id = quote(str(args["table_id"]), safe="")
    row_id = quote(str(args["row_id"]), safe="")

    r = requests.get(
        f"{cfg.base_url}/api/database/rows/table/{table_id}/{row_id}/",
        headers=auth,
        params=_query(args, ("include", "user_field_names", "view")),
        timeout=10,
    )
    if r.status_code == 404:
        e = _error(r)
        if e == "ERROR_TABLE_DOES_NOT_EXIST":
            return ("rejected", {"code": "table_not_found", "detail": _detail(r)})
        if e == "ERROR_ROW_DOES_NOT_EXIST":
            return ("rejected", {"code": "row_not_found", "detail": _detail(r)})
        if e == "ERROR_VIEW_DOES_NOT_EXIST":
            return ("rejected", {"code": "view_not_found", "detail": _detail(r)})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": _detail(r)})
    return ("resolved", r.json())
```


### 4.3 row.create

| | |
|---|---|
| **Documentation** | [Create row](https://api.baserow.io/api/redoc/#tag/Database-table-rows/operation/create_database_table_row) |

```json
{ "func": "row.create", "args": { ... } }
```

### 4.3.1 Promise Param Schema

```json
{
  "description": "Create one row. Resolves with the created row. Rejects table_not_found, row_not_found (the before row) or view_not_found when a named table, row or view does not exist, and invalid_request when a value is not accepted by its field. Baserow accepts no client-supplied row identity, so a re-delivery of this promise creates a second row; use rows.update for a write that must survive re-delivery unchanged. Keys the table has no field for are dropped silently and the row is created with those fields at their defaults — read field.list first and send only names it returns.",
  "type": "object",
  "properties": {
    "table_id": {
      "type": "integer"
    },
    "values": {
      "type": "object",
      "description": "Field values. Keys are field_{id}, or the field's name when user_field_names is true. Value shape depends on the field type — see field.list. A field left out takes null, false or its configured default.",
      "additionalProperties": true
    },
    "before": {
      "type": "integer",
      "description": "Position the new row before this row id. Appended to the end when omitted."
    },
    "user_field_names": {
      "type": "boolean",
      "description": "true makes values and the response keyed by the user-visible field names instead of field_{id}. false when omitted."
    },
    "view": {
      "type": "integer",
      "description": "Create the row as seen through this view: the view's field permissions and default values apply."
    },
    "send_webhook_events": {
      "type": "boolean",
      "description": "false suppresses the table's row-created webhooks for this write. true when omitted."
    }
  },
  "required": [
    "table_id",
    "values"
  ],
  "additionalProperties": false
}
```

### 4.3.2 Promise Value Schema

#### Resolved

Same as 4.2.2.

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["table_not_found", "row_not_found", "view_not_found", "invalid_request"]
    },
    "detail": {
      "description": "= response.body.detail of the 400/404; for ERROR_REQUEST_BODY_VALIDATION an object keyed by field name"
    }
  },
  "required": ["code"]
}
```

### 4.3.3 Integration Request

```
POST /api/database/rows/table/{promise.param.table_id}/{?before,user_field_names,view,send_webhook_events = promise.param.*} → 200
```

```json
{
  "type": "object",
  "description": "= promise.param.values",
  "additionalProperties": true
}
```

### 4.3.4 Integration Response

Same as 4.2.4.

### 4.3.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_response` |

```python
def row_create(cfg, promise):
    auth = _token(cfg)
    args = promise.param["args"]
    table_id = quote(str(args["table_id"]), safe="")

    # Unkeyed POST: a re-delivery costs one extra row in the table, and the
    # promise resolves with the second one.
    r = requests.post(
        f"{cfg.base_url}/api/database/rows/table/{table_id}/",
        headers=auth,
        params=_query(args, ("before", "user_field_names", "view", "send_webhook_events")),
        json=args["values"],
        timeout=10,
    )
    if r.status_code == 404:
        e = _error(r)
        if e == "ERROR_TABLE_DOES_NOT_EXIST":
            return ("rejected", {"code": "table_not_found", "detail": _detail(r)})
        if e == "ERROR_ROW_DOES_NOT_EXIST":
            return ("rejected", {"code": "row_not_found", "detail": _detail(r)})
        if e == "ERROR_VIEW_DOES_NOT_EXIST":
            return ("rejected", {"code": "view_not_found", "detail": _detail(r)})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": _detail(r)})
    return ("resolved", r.json())
```


### 4.4 row.update

| | |
|---|---|
| **Documentation** | [Update row](https://api.baserow.io/api/redoc/#tag/Database-table-rows/operation/update_database_table_row) |

```json
{ "func": "row.update", "args": { ... } }
```

### 4.4.1 Promise Param Schema

```json
{
  "description": "Set field values on one row. Resolves with the updated row. Rejects table_not_found, row_not_found or view_not_found when the named table, row or view does not exist, and invalid_request when a value is not accepted by its field. The write is a function of the promise, so a re-delivery re-applies the same values and leaves the same row state.",
  "type": "object",
  "properties": {
    "table_id": {
      "type": "integer"
    },
    "row_id": {
      "type": "integer"
    },
    "values": {
      "type": "object",
      "description": "Field values to set. Keys are field_{id}, or the field's name when user_field_names is true. A field left out is not touched.",
      "additionalProperties": true
    },
    "user_field_names": {
      "type": "boolean",
      "description": "true makes values and the response keyed by the user-visible field names instead of field_{id}. false when omitted."
    },
    "view": {
      "type": "integer",
      "description": "Update the row as seen through this view: the view's field permissions and default values apply."
    },
    "send_webhook_events": {
      "type": "boolean",
      "description": "false suppresses the table's row-updated webhooks for this write. true when omitted."
    }
  },
  "required": [
    "table_id",
    "row_id",
    "values"
  ],
  "additionalProperties": false
}
```

### 4.4.2 Promise Value Schema

#### Resolved

Same as 4.2.2.

#### Rejected

Same as 4.3.2.

### 4.4.3 Integration Request

```
PATCH /api/database/rows/table/{promise.param.table_id}/{promise.param.row_id}/{?user_field_names,view,send_webhook_events = promise.param.*} → 200
```

```json
{
  "type": "object",
  "description": "= promise.param.values",
  "additionalProperties": true
}
```

### 4.4.4 Integration Response

Same as 4.2.4.

### 4.4.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_response` |

```python
def row_update(cfg, promise):
    auth = _token(cfg)
    args = promise.param["args"]
    table_id = quote(str(args["table_id"]), safe="")
    row_id = quote(str(args["row_id"]), safe="")

    # A repeat of this PATCH writes the same values to the same row, so a
    # re-delivery costs one request and changes nothing.
    r = requests.patch(
        f"{cfg.base_url}/api/database/rows/table/{table_id}/{row_id}/",
        headers=auth,
        params=_query(args, ("user_field_names", "view", "send_webhook_events")),
        json=args["values"],
        timeout=10,
    )
    if r.status_code == 404:
        e = _error(r)
        if e == "ERROR_TABLE_DOES_NOT_EXIST":
            return ("rejected", {"code": "table_not_found", "detail": _detail(r)})
        if e == "ERROR_ROW_DOES_NOT_EXIST":
            return ("rejected", {"code": "row_not_found", "detail": _detail(r)})
        if e == "ERROR_VIEW_DOES_NOT_EXIST":
            return ("rejected", {"code": "view_not_found", "detail": _detail(r)})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": _detail(r)})
    return ("resolved", r.json())
```


### 4.5 row.delete

| | |
|---|---|
| **Documentation** | [Delete row](https://api.baserow.io/api/redoc/#tag/Database-table-rows/operation/delete_database_table_row) |

```json
{ "func": "row.delete", "args": { ... } }
```

### 4.5.1 Promise Param Schema

```json
{
  "description": "Delete one row. The row is moved to the workspace trash, not erased. Resolves with an empty object. Rejects table_not_found or view_not_found when the named table or view does not exist, already_deleted when the row is already in the trash, and row_not_found when the row does not exist — including a re-delivery of this promise after an earlier attempt already deleted the row.",
  "type": "object",
  "properties": {
    "table_id": {
      "type": "integer"
    },
    "row_id": {
      "type": "integer"
    },
    "view": {
      "type": "integer",
      "description": "Delete the row as seen through this view: the view's field permissions apply."
    },
    "send_webhook_events": {
      "type": "boolean",
      "description": "false suppresses the table's row-deleted webhooks for this write. true when omitted."
    }
  },
  "required": [
    "table_id",
    "row_id"
  ],
  "additionalProperties": false
}
```

### 4.5.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "properties": {},
  "description": "Empty. The endpoint answers 204 with no body."
}
```

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["table_not_found", "row_not_found", "view_not_found", "already_deleted", "invalid_request"]
    },
    "detail": {
      "description": "= response.body.detail of the 400/404"
    }
  },
  "required": ["code"]
}
```

### 4.5.3 Integration Request

```
DELETE /api/database/rows/table/{promise.param.table_id}/{promise.param.row_id}/{?view,send_webhook_events = promise.param.*} → 204
```

### 4.5.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "error": {
      "type": "string",
      "description": "Present on 4xx only. Machine readable failure code."
    },
    "detail": {
      "description": "Present on 4xx only. String or object."
    }
  },
  "description": "Empty on 204."
}
```

### 4.5.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_response` |

```python
def row_delete(cfg, promise):
    auth = _token(cfg)
    args = promise.param["args"]
    table_id = quote(str(args["table_id"]), safe="")
    row_id = quote(str(args["row_id"]), safe="")

    r = requests.delete(
        f"{cfg.base_url}/api/database/rows/table/{table_id}/{row_id}/",
        headers=auth,
        params=_query(args, ("view", "send_webhook_events")),
        timeout=10,
    )
    if r.status_code == 404:
        e = _error(r)
        if e == "ERROR_TABLE_DOES_NOT_EXIST":
            return ("rejected", {"code": "table_not_found", "detail": _detail(r)})
        if e == "ERROR_ROW_DOES_NOT_EXIST":
            return ("rejected", {"code": "row_not_found", "detail": _detail(r)})
        if e == "ERROR_VIEW_DOES_NOT_EXIST":
            return ("rejected", {"code": "view_not_found", "detail": _detail(r)})
    if r.status_code == 400 and _error(r) == "ERROR_CANNOT_DELETE_ALREADY_DELETED_ITEM":
        return ("rejected", {"code": "already_deleted", "detail": _detail(r)})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": _detail(r)})
    return ("resolved", {})
```


### 4.6 row.move

| | |
|---|---|
| **Documentation** | [Move row](https://api.baserow.io/api/redoc/#tag/Database-table-rows/operation/move_database_table_row) |

```json
{ "func": "row.move", "args": { ... } }
```

### 4.6.1 Promise Param Schema

```json
{
  "description": "Move one row to another position in the table. Resolves with the moved row. Rejects table_not_found, row_not_found or view_not_found when the named table, row or view does not exist, and invalid_request on a malformed query. A re-delivery repeats the same move and leaves the same order.",
  "type": "object",
  "properties": {
    "table_id": {
      "type": "integer"
    },
    "row_id": {
      "type": "integer"
    },
    "before_id": {
      "type": "integer",
      "description": "Place the row immediately before this row id. Moves the row to the end when omitted."
    },
    "user_field_names": {
      "type": "boolean",
      "description": "true makes the response keys the user-visible field names instead of field_{id}. false when omitted."
    },
    "view": {
      "type": "integer",
      "description": "Move the row as seen through this view: the view's field permissions apply."
    },
    "send_webhook_events": {
      "type": "boolean",
      "description": "false suppresses the table's webhooks for this write. true when omitted."
    }
  },
  "required": [
    "table_id",
    "row_id"
  ],
  "additionalProperties": false
}
```

### 4.6.2 Promise Value Schema

#### Resolved

Same as 4.2.2.

#### Rejected

Same as 4.2.2.

### 4.6.3 Integration Request

```
PATCH /api/database/rows/table/{promise.param.table_id}/{promise.param.row_id}/move/{?before_id,user_field_names,view,send_webhook_events = promise.param.*} → 200
```

### 4.6.4 Integration Response

Same as 4.2.4.

### 4.6.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_response` |

```python
def row_move(cfg, promise):
    auth = _token(cfg)
    args = promise.param["args"]
    table_id = quote(str(args["table_id"]), safe="")
    row_id = quote(str(args["row_id"]), safe="")

    # Moving a row already at the requested position is a no-op, so a
    # re-delivery costs one request and changes nothing.
    r = requests.patch(
        f"{cfg.base_url}/api/database/rows/table/{table_id}/{row_id}/move/",
        headers=auth,
        params=_query(args, ("before_id", "user_field_names", "view", "send_webhook_events")),
        timeout=10,
    )
    if r.status_code == 404:
        e = _error(r)
        if e == "ERROR_TABLE_DOES_NOT_EXIST":
            return ("rejected", {"code": "table_not_found", "detail": _detail(r)})
        if e == "ERROR_ROW_DOES_NOT_EXIST":
            return ("rejected", {"code": "row_not_found", "detail": _detail(r)})
        if e == "ERROR_VIEW_DOES_NOT_EXIST":
            return ("rejected", {"code": "view_not_found", "detail": _detail(r)})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": _detail(r)})
    return ("resolved", r.json())
```


### 4.7 rows.create

| | |
|---|---|
| **Documentation** | [Create rows](https://api.baserow.io/api/redoc/#tag/Database-table-rows/operation/batch_create_database_table_rows) |

```json
{ "func": "rows.create", "args": { ... } }
```

### 4.7.1 Promise Param Schema

```json
{
  "description": "Create up to 200 rows in one write. Resolves with the created rows. Rejects table_not_found, row_not_found (the before row) or view_not_found when a named table, row or view does not exist, and invalid_request when a value is not accepted by its field or more than BATCH_ROWS_SIZE_LIMIT rows are sent. Baserow accepts no client-supplied row identity, so a re-delivery of this promise creates a second set of rows; for more rows than the batch limit, or for a bulk write that must survive re-delivery, use table.import. This endpoint does not fire row-created webhooks.",
  "type": "object",
  "properties": {
    "table_id": {
      "type": "integer"
    },
    "rows": {
      "type": "array",
      "minItems": 1,
      "maxItems": 200,
      "description": "One field-value object per row, shaped as row.create's values. Capped by BATCH_ROWS_SIZE_LIMIT, 200 by default.",
      "items": {
        "type": "object",
        "additionalProperties": true
      }
    },
    "before": {
      "type": "integer",
      "description": "Position the new rows before this row id. Appended to the end when omitted."
    },
    "include_metadata": {
      "type": "boolean",
      "description": "true adds a metadata object naming the field ids the write changed, including cascading updates. false when omitted."
    },
    "user_field_names": {
      "type": "boolean",
      "description": "true makes rows and the response keyed by the user-visible field names instead of field_{id}. false when omitted."
    },
    "view": {
      "type": "integer",
      "description": "Create the rows as seen through this view: the view's field permissions and default values apply."
    },
    "send_webhook_events": {
      "type": "boolean",
      "description": "false suppresses the table's webhooks for this write. true when omitted."
    }
  },
  "required": [
    "table_id",
    "rows"
  ],
  "additionalProperties": false
}
```

### 4.7.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "description": "= response.body"
}
```

#### Rejected

Same as 4.3.2.

### 4.7.3 Integration Request

```
POST /api/database/rows/table/{promise.param.table_id}/batch/{?before,include_metadata,user_field_names,view,send_webhook_events = promise.param.*} → 200
```

```json
{
  "type": "object",
  "properties": {
    "items": {
      "type": "array",
      "description": "= promise.param.rows",
      "items": {
        "type": "object",
        "additionalProperties": true
      }
    }
  },
  "required": [
    "items"
  ],
  "additionalProperties": false
}
```

### 4.7.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "items": {
      "type": "array",
      "items": {
        "description": "Same as 4.2.4"
      }
    },
    "metadata": {
      "type": "object",
      "description": "Present when include_metadata is true. Carries update_field_ids and cascade_update.",
      "additionalProperties": true
    },
    "error": {
      "type": "string",
      "description": "Present on 4xx only. Machine readable failure code."
    },
    "detail": {
      "description": "Present on 4xx only. String or object."
    }
  },
  "required": ["items"]
}
```

### 4.7.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_response` |

```python
def rows_create(cfg, promise):
    auth = _token(cfg)
    args = promise.param["args"]
    table_id = quote(str(args["table_id"]), safe="")

    # Unkeyed POST: a re-delivery costs one extra copy of every row.
    r = requests.post(
        f"{cfg.base_url}/api/database/rows/table/{table_id}/batch/",
        headers=auth,
        params=_query(
            args,
            ("before", "include_metadata", "user_field_names", "view", "send_webhook_events"),
        ),
        json={"items": args["rows"]},
        timeout=10,
    )
    if r.status_code == 404:
        e = _error(r)
        if e == "ERROR_TABLE_DOES_NOT_EXIST":
            return ("rejected", {"code": "table_not_found", "detail": _detail(r)})
        if e == "ERROR_ROW_DOES_NOT_EXIST":
            return ("rejected", {"code": "row_not_found", "detail": _detail(r)})
        if e == "ERROR_VIEW_DOES_NOT_EXIST":
            return ("rejected", {"code": "view_not_found", "detail": _detail(r)})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": _detail(r)})
    return ("resolved", r.json())
```


### 4.8 rows.update

| | |
|---|---|
| **Documentation** | [Update rows](https://api.baserow.io/api/redoc/#tag/Database-table-rows/operation/batch_update_database_table_rows) |

```json
{ "func": "rows.update", "args": { ... } }
```

### 4.8.1 Promise Param Schema

```json
{
  "description": "Set field values on up to 200 identified rows in one write. Resolves with the updated rows. Rejects table_not_found, row_not_found or view_not_found when a named table, row or view does not exist, row_ids_not_unique when one row id appears twice, and invalid_request when a value is not accepted by its field or more than BATCH_ROWS_SIZE_LIMIT rows are sent. Every row carries its own id, so the write is a function of the promise and a re-delivery re-applies the same values to the same rows. This endpoint does not fire row-updated webhooks.",
  "type": "object",
  "properties": {
    "table_id": {
      "type": "integer"
    },
    "rows": {
      "type": "array",
      "minItems": 1,
      "maxItems": 200,
      "description": "One object per row. Each carries id plus the field values to set, keyed as row.update's values. Capped by BATCH_ROWS_SIZE_LIMIT, 200 by default.",
      "items": {
        "type": "object",
        "properties": {
          "id": {
            "type": "integer",
            "description": "Row to update."
          }
        },
        "required": ["id"],
        "additionalProperties": true
      }
    },
    "include_metadata": {
      "type": "boolean",
      "description": "true adds a metadata object naming the field ids the write changed, including cascading updates. false when omitted."
    },
    "user_field_names": {
      "type": "boolean",
      "description": "true makes rows and the response keyed by the user-visible field names instead of field_{id}. false when omitted."
    },
    "view": {
      "type": "integer",
      "description": "Update the rows as seen through this view: the view's field permissions and default values apply."
    },
    "send_webhook_events": {
      "type": "boolean",
      "description": "false suppresses the table's webhooks for this write. true when omitted."
    }
  },
  "required": [
    "table_id",
    "rows"
  ],
  "additionalProperties": false
}
```

### 4.8.2 Promise Value Schema

#### Resolved

Same as 4.7.2.

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["table_not_found", "row_not_found", "view_not_found", "row_ids_not_unique", "invalid_request"]
    },
    "detail": {
      "description": "= response.body.detail of the 400/404"
    }
  },
  "required": ["code"]
}
```

### 4.8.3 Integration Request

```
PATCH /api/database/rows/table/{promise.param.table_id}/batch/{?include_metadata,user_field_names,view,send_webhook_events = promise.param.*} → 200
```

```json
{
  "type": "object",
  "properties": {
    "items": {
      "type": "array",
      "description": "= promise.param.rows",
      "items": {
        "type": "object",
        "additionalProperties": true
      }
    }
  },
  "required": [
    "items"
  ],
  "additionalProperties": false
}
```

### 4.8.4 Integration Response

Same as 4.7.4.

### 4.8.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_response` |

```python
def rows_update(cfg, promise):
    auth = _token(cfg)
    args = promise.param["args"]
    table_id = quote(str(args["table_id"]), safe="")

    # Every item names the row it writes to, so a re-delivery writes the same
    # values to the same rows and changes nothing.
    r = requests.patch(
        f"{cfg.base_url}/api/database/rows/table/{table_id}/batch/",
        headers=auth,
        params=_query(
            args,
            ("include_metadata", "user_field_names", "view", "send_webhook_events"),
        ),
        json={"items": args["rows"]},
        timeout=10,
    )
    if r.status_code == 404:
        e = _error(r)
        if e == "ERROR_TABLE_DOES_NOT_EXIST":
            return ("rejected", {"code": "table_not_found", "detail": _detail(r)})
        if e == "ERROR_ROW_DOES_NOT_EXIST":
            return ("rejected", {"code": "row_not_found", "detail": _detail(r)})
        if e == "ERROR_VIEW_DOES_NOT_EXIST":
            return ("rejected", {"code": "view_not_found", "detail": _detail(r)})
    if r.status_code == 400 and _error(r) == "ERROR_ROW_IDS_NOT_UNIQUE":
        return ("rejected", {"code": "row_ids_not_unique", "detail": _detail(r)})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": _detail(r)})
    return ("resolved", r.json())
```


### 4.9 rows.delete

| | |
|---|---|
| **Documentation** | [Delete rows](https://api.baserow.io/api/redoc/#tag/Database-table-rows/operation/batch_delete_database_table_rows) |

```json
{ "func": "rows.delete", "args": { ... } }
```

### 4.9.1 Promise Param Schema

```json
{
  "description": "Delete up to 200 rows in one write. The rows are moved to the workspace trash, not erased. Resolves with an empty object. Rejects table_not_found or view_not_found when the named table or view does not exist, row_ids_not_unique when one row id appears twice, already_deleted when a row is already in the trash, and row_not_found when any listed row does not exist — including a re-delivery of this promise after an earlier attempt already deleted them. No row is deleted when any id in the list fails. This endpoint does not fire row-deleted webhooks.",
  "type": "object",
  "properties": {
    "table_id": {
      "type": "integer"
    },
    "row_ids": {
      "type": "array",
      "minItems": 1,
      "maxItems": 200,
      "description": "Rows to delete. Capped by BATCH_ROWS_SIZE_LIMIT, 200 by default.",
      "items": {
        "type": "integer"
      }
    },
    "view": {
      "type": "integer",
      "description": "Delete the rows as seen through this view: the view's field permissions apply."
    },
    "send_webhook_events": {
      "type": "boolean",
      "description": "false suppresses the table's webhooks for this write. true when omitted."
    }
  },
  "required": [
    "table_id",
    "row_ids"
  ],
  "additionalProperties": false
}
```

### 4.9.2 Promise Value Schema

#### Resolved

Same as 4.5.2.

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["table_not_found", "row_not_found", "view_not_found", "row_ids_not_unique", "already_deleted", "invalid_request"]
    },
    "detail": {
      "description": "= response.body.detail of the 400/404"
    }
  },
  "required": ["code"]
}
```

### 4.9.3 Integration Request

```
POST /api/database/rows/table/{promise.param.table_id}/batch-delete/{?view,send_webhook_events = promise.param.*} → 204
```

```json
{
  "type": "object",
  "properties": {
    "items": {
      "type": "array",
      "description": "= promise.param.row_ids",
      "items": {
        "type": "integer"
      }
    }
  },
  "required": [
    "items"
  ],
  "additionalProperties": false
}
```

### 4.9.4 Integration Response

Same as 4.5.4.

### 4.9.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_response` |

```python
def rows_delete(cfg, promise):
    auth = _token(cfg)
    args = promise.param["args"]
    table_id = quote(str(args["table_id"]), safe="")

    r = requests.post(
        f"{cfg.base_url}/api/database/rows/table/{table_id}/batch-delete/",
        headers=auth,
        params=_query(args, ("view", "send_webhook_events")),
        json={"items": args["row_ids"]},
        timeout=10,
    )
    if r.status_code == 404:
        e = _error(r)
        if e == "ERROR_TABLE_DOES_NOT_EXIST":
            return ("rejected", {"code": "table_not_found", "detail": _detail(r)})
        if e == "ERROR_ROW_DOES_NOT_EXIST":
            return ("rejected", {"code": "row_not_found", "detail": _detail(r)})
        if e == "ERROR_VIEW_DOES_NOT_EXIST":
            return ("rejected", {"code": "view_not_found", "detail": _detail(r)})
    if r.status_code == 400:
        e = _error(r)
        if e == "ERROR_ROW_IDS_NOT_UNIQUE":
            return ("rejected", {"code": "row_ids_not_unique", "detail": _detail(r)})
        if e == "ERROR_CANNOT_DELETE_ALREADY_DELETED_ITEM":
            return ("rejected", {"code": "already_deleted", "detail": _detail(r)})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": _detail(r)})
    return ("resolved", {})
```


### 4.10 export.create

| | |
|---|---|
| **Documentation** | [Export table](https://api.baserow.io/api/redoc/#tag/Database-table-export/operation/export_table) |

```json
{ "func": "export.create", "args": { ... } }
```

### 4.10.1 Promise Param Schema

```json
{
  "description": "Export a table, or one of its views, to a file and observe the job to a terminal state. Resolves on state \"finished\" with the job carrying url and exported_file_name. Rejects table_not_found or view_not_found when the named table or view does not exist, invalid_request when a filter, sort or exporter option is not accepted, export_failed on state \"failed\", cancelled on state \"cancelled\", expired on state \"expired\", and job_not_found when the job disappears before finishing. Creating an export job sets every other unfinished export job of the same account to \"cancelled\", so two of these promises running concurrently on one account cancel each other. The exported file is deleted and the job set to \"expired\" EXPORT_FILE_EXPIRE_MINUTES (60) after the job was created, so the caller must fetch url within that hour. Duration is the table's own export time — seconds for a small table, minutes for a large one, plus any wait behind other jobs on the export worker; size timeoutAt accordingly. Only the csv exporter is registered without a licence; json, xml, excel and file answer ERROR_FEATURE_NOT_AVAILABLE.",
  "type": "object",
  "properties": {
    "table_id": {
      "type": "integer"
    },
    "exporter_type": {
      "type": "string",
      "enum": ["csv", "json", "xml", "excel", "file"],
      "description": "The file type to export to."
    },
    "view_id": {
      "type": ["integer", "null"],
      "minimum": 0,
      "description": "Export this view instead of the whole table, using its filters, sorts and view-specific settings. Enumerate via view.list."
    },
    "export_charset": {
      "type": "string",
      "enum": ["utf-8", "iso-8859-6", "windows-1256", "iso-8859-4", "windows-1257", "iso-8859-14", "iso-8859-2", "windows-1250", "gbk", "gb18030", "big5", "koi8-r", "koi8-u", "iso-8859-5", "windows-1251", "x-mac-cyrillic", "iso-8859-7", "windows-1253", "iso-8859-8", "windows-1255", "euc-jp", "iso-2022-jp", "shift-jis", "euc-kr", "macintosh", "iso-8859-10", "iso-8859-16", "windows-874", "windows-1254", "windows-1258", "iso-8859-1", "windows-1252", "iso-8859-3"],
      "description": "Character set of the exported file. utf-8 when omitted."
    },
    "filters": {
      "type": ["object", "null"],
      "description": "Filter tree applied to the export. field is a field id.",
      "properties": {
        "filter_type": {
          "type": "string",
          "enum": ["AND", "OR"]
        },
        "filters": {
          "type": "array",
          "items": {
            "type": "object",
            "additionalProperties": true
          }
        }
      },
      "required": ["filter_type"],
      "additionalProperties": true
    },
    "order_by": {
      "type": ["string", "null"],
      "description": "Field ids separated by commas; a leading \"-\" orders that field descending."
    },
    "fields": {
      "type": ["array", "null"],
      "description": "Field ids to include, in the order they should appear.",
      "items": {
        "type": "integer"
      }
    },
    "include_row_id": {
      "type": "boolean",
      "description": "Include the row id column. true when omitted."
    },
    "include_primary_field": {
      "type": "boolean",
      "description": "Include the primary field column. true when omitted."
    },
    "csv_column_separator": {
      "type": "string",
      "enum": [",", ";", "|", "tab", "record_separator", "unit_separator"],
      "description": "exporter_type csv only. \",\" when omitted."
    },
    "csv_include_header": {
      "type": "boolean",
      "description": "exporter_type csv only. Write a header row. true when omitted."
    },
    "excel_include_header": {
      "type": "boolean",
      "description": "exporter_type excel only. Write the field names as a header row. true when omitted."
    },
    "organize_files": {
      "type": "boolean",
      "description": "exporter_type file only. Group the exported files by row id. true when omitted."
    }
  },
  "required": [
    "table_id",
    "exporter_type"
  ],
  "additionalProperties": false
}
```

### 4.10.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "integer",
      "description": "= response.body.id"
    },
    "table": {
      "type": ["integer", "null"],
      "description": "= response.body.table"
    },
    "view": {
      "type": ["integer", "null"],
      "description": "= response.body.view"
    },
    "exporter_type": {
      "type": "string",
      "description": "= response.body.exporter_type"
    },
    "state": {
      "type": "string",
      "description": "= response.body.state"
    },
    "exported_file_name": {
      "type": ["string", "null"],
      "description": "= response.body.exported_file_name"
    },
    "created_at": {
      "type": "string",
      "description": "= response.body.created_at"
    },
    "progress_percentage": {
      "type": "number",
      "description": "= response.body.progress_percentage"
    },
    "url": {
      "type": ["string", "null"],
      "description": "= response.body.url"
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
      "enum": ["table_not_found", "view_not_found", "job_not_found", "invalid_request", "export_failed", "cancelled", "expired"]
    },
    "detail": {
      "description": "table_not_found / view_not_found / job_not_found / invalid_request: = response.body.detail of the 400/404; export_failed / cancelled / expired: = response.body (the terminal export job object)"
    }
  },
  "required": ["code"]
}
```

### 4.10.3 Integration Request

```
POST /api/database/export/table/{promise.param.table_id}/ → 200
```

```json
{
  "type": "object",
  "properties": {
    "exporter_type": {
      "type": "string",
      "description": "= promise.param.exporter_type"
    },
    "view_id": {
      "type": ["integer", "null"],
      "description": "= promise.param.view_id"
    },
    "export_charset": {
      "type": "string",
      "description": "= promise.param.export_charset"
    },
    "filters": {
      "type": ["object", "null"],
      "description": "= promise.param.filters"
    },
    "order_by": {
      "type": ["string", "null"],
      "description": "= promise.param.order_by"
    },
    "fields": {
      "type": ["array", "null"],
      "description": "= promise.param.fields"
    },
    "include_row_id": {
      "type": "boolean",
      "description": "= promise.param.include_row_id"
    },
    "include_primary_field": {
      "type": "boolean",
      "description": "= promise.param.include_primary_field"
    },
    "csv_column_separator": {
      "type": "string",
      "description": "= promise.param.csv_column_separator"
    },
    "csv_include_header": {
      "type": "boolean",
      "description": "= promise.param.csv_include_header"
    },
    "excel_include_header": {
      "type": "boolean",
      "description": "= promise.param.excel_include_header"
    },
    "organize_files": {
      "type": "boolean",
      "description": "= promise.param.organize_files"
    }
  },
  "required": [
    "exporter_type"
  ],
  "additionalProperties": false
}
```

### 4.10.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "integer",
      "description": "Export job id; the argument of export.get."
    },
    "table": {
      "type": ["integer", "null"]
    },
    "view": {
      "type": ["integer", "null"]
    },
    "exporter_type": {
      "type": "string"
    },
    "state": {
      "type": "string",
      "enum": ["pending", "exporting", "cancelled", "finished", "failed", "expired"],
      "description": "finished, failed, cancelled and expired are terminal; pending and exporting are running."
    },
    "status": {
      "type": "string",
      "description": "Deprecated duplicate of state."
    },
    "exported_file_name": {
      "type": ["string", "null"],
      "description": "Non-null once the file exists."
    },
    "created_at": {
      "type": "string",
      "description": "ISO 8601. The file is deleted and the job set to expired EXPORT_FILE_EXPIRE_MINUTES (60) after this instant."
    },
    "progress_percentage": {
      "type": "number",
      "description": "0 to 100."
    },
    "url": {
      "type": ["string", "null"],
      "description": "Absolute URL of the exported file; non-null once the file exists."
    },
    "error": {
      "type": "string",
      "description": "Present on 4xx only. Machine readable failure code."
    },
    "detail": {
      "description": "Present on 4xx only. String or object."
    }
  },
  "required": ["id", "exporter_type", "state", "status", "created_at", "url"]
}
```

### 4.10.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_poll` |

```python
def export_create(cfg, promise):
    auth = _token(cfg)
    args = promise.param["args"]
    table_id = quote(str(args["table_id"]), safe="")

    # The export job carries no client-supplied identity and no listing
    # endpoint exposes it, so a re-delivery costs a second export — and,
    # because creating a job sets this account's other unfinished export
    # jobs to "cancelled", it also cancels the job the first attempt made.
    r = requests.post(
        f"{cfg.base_url}/api/database/export/table/{table_id}/",
        headers=auth,
        json={k: v for k, v in args.items() if k != "table_id"},
        timeout=10,
    )
    if r.status_code == 404:
        e = _error(r)
        if e == "ERROR_TABLE_DOES_NOT_EXIST":
            return ("rejected", {"code": "table_not_found", "detail": _detail(r)})
        if e == "ERROR_VIEW_DOES_NOT_EXIST":
            return ("rejected", {"code": "view_not_found", "detail": _detail(r)})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": _detail(r)})

    job = r.json()
    job_id = quote(str(job["id"]), safe="")
    failures = 0
    while job["state"] not in EXPORT_TERMINAL:
        if time.time() * 1000 >= promise.timeout_at:
            raise Exception("release", "promise timed out")
        time.sleep(
            min(
                cfg.poll_export.total_seconds(),
                (promise.timeout_at - time.time() * 1000) / 1000,
            )
        )
        try:
            g = requests.get(
                f"{cfg.base_url}/api/database/export/{job_id}/",
                headers=auth,
                timeout=10,
            )
            if g.status_code == 401:
                auth = _token(cfg)
                continue
            if g.status_code == 404:
                return ("rejected", {"code": "job_not_found", "detail": _detail(g)})
            _check(g)
            if g.status_code >= 400:
                return ("rejected", {"code": "invalid_request", "detail": _detail(g)})
            job = g.json()
            failures = 0
        except Exception as exc:
            if exc.args[:1] == ("halt",):
                raise
            failures += 1
            if failures >= 5:
                raise

    keys = (
        "id",
        "table",
        "view",
        "exporter_type",
        "state",
        "exported_file_name",
        "created_at",
        "progress_percentage",
        "url",
    )
    if job["state"] == "finished":
        return ("resolved", {k: job[k] for k in keys})  # the 4.10.2 Resolved mapping
    if job["state"] == "cancelled":
        return ("rejected", {"code": "cancelled", "detail": job})
    if job["state"] == "expired":
        return ("rejected", {"code": "expired", "detail": job})
    return ("rejected", {"code": "export_failed", "detail": job})
```


### 4.11 export.get

| | |
|---|---|
| **Documentation** | [Get export job](https://api.baserow.io/api/redoc/#tag/Database-table-export/operation/get_export_job) |

```json
{ "func": "export.get", "args": { ... } }
```

### 4.11.1 Promise Param Schema

```json
{
  "description": "Read one export job — its state, progress and, once the file exists, its url. Resolves with the job. Rejects job_not_found when no such job belongs to this account, and invalid_request on a malformed request. A plain read — not the completion mechanism; export.create observes independently.",
  "type": "object",
  "properties": {
    "job_id": {
      "type": "integer",
      "description": "Export job id, as returned by export.create."
    }
  },
  "required": [
    "job_id"
  ],
  "additionalProperties": false
}
```

### 4.11.2 Promise Value Schema

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
      "enum": ["job_not_found", "invalid_request"]
    },
    "detail": {
      "description": "= response.body.detail of the 400/404"
    }
  },
  "required": ["code"]
}
```

### 4.11.3 Integration Request

```
GET /api/database/export/{promise.param.job_id}/ → 200
```

### 4.11.4 Integration Response

Same as 4.10.4.

### 4.11.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def export_get(cfg, promise):
    auth = _token(cfg)
    args = promise.param["args"]
    job_id = quote(str(args["job_id"]), safe="")

    r = requests.get(
        f"{cfg.base_url}/api/database/export/{job_id}/",
        headers=auth,
        timeout=10,
    )
    if r.status_code == 404:
        return ("rejected", {"code": "job_not_found", "detail": _detail(r)})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": _detail(r)})
    return ("resolved", r.json())
```


### 4.12 table.import

| | |
|---|---|
| **Documentation** | [Import data into table](https://api.baserow.io/api/redoc/#tag/Database-tables/operation/import_data_database_table_async) |

```json
{ "func": "table.import", "args": { ... } }
```

### 4.12.1 Promise Param Schema

```json
{
  "description": "Bulk-load rows into an existing table and observe the import job to a terminal state. This is the only write that takes more rows than BATCH_ROWS_SIZE_LIMIT in one unit of work. Resolves on state \"finished\" with the job, including its report: a job reaches \"finished\" even when individual rows were rejected, and report.failing_rows names them by their index in data. Rejects table_not_found when the table does not exist, invalid_request when the body is not accepted (data must hold at least one row), import_failed on state \"failed\", cancelled on state \"cancelled\", and job_not_found when the job disappears before finishing. Duration is the size of data — seconds for hundreds of rows, minutes for hundreds of thousands, plus any wait behind other jobs on the worker; BASEROW_JOB_SOFT_TIME_LIMIT (30 minutes) bounds one job. Size timeoutAt accordingly.",
  "type": "object",
  "properties": {
    "table_id": {
      "type": "integer",
      "description": "Table to load into. The table must already exist with the fields the data fills."
    },
    "data": {
      "type": "array",
      "minItems": 1,
      "description": "One array per row, values ordered by the table's writable fields — see field.list. Values must be compatible with their field's type; a value that is not is reported in report.failing_rows and its row is not created.",
      "items": {
        "type": "array",
        "items": {}
      }
    },
    "configuration": {
      "type": "object",
      "description": "Upsert configuration. upsert_fields is a list of field ids identifying rows; upsert_values is one list of matching values per row of data; skipped_fields is a list of field ids an upsert must not overwrite.",
      "properties": {
        "upsert_fields": {
          "type": ["array", "null"],
          "minItems": 1,
          "items": {
            "type": "integer",
            "minimum": 1
          }
        },
        "upsert_values": {
          "type": ["array", "null"],
          "items": {
            "type": "array",
            "minItems": 1,
            "items": {}
          }
        },
        "skipped_fields": {
          "type": ["array", "null"],
          "items": {
            "type": "integer",
            "minimum": 1
          }
        }
      },
      "additionalProperties": false
    },
    "importer_type": {
      "type": "string",
      "maxLength": 32,
      "description": "Identifier of the importer that parsed the source file. Empty when omitted."
    }
  },
  "required": [
    "table_id",
    "data"
  ],
  "additionalProperties": false
}
```

### 4.12.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "integer",
      "description": "= response.body.id"
    },
    "type": {
      "type": "string",
      "description": "= response.body.type"
    },
    "state": {
      "type": "string",
      "description": "= response.body.state"
    },
    "progress_percentage": {
      "type": "integer",
      "description": "= response.body.progress_percentage"
    },
    "table_id": {
      "type": "integer",
      "description": "= response.body.table_id"
    },
    "database_id": {
      "type": "integer",
      "description": "= response.body.database_id"
    },
    "original_file_name": {
      "type": "string",
      "description": "= response.body.original_file_name (equals sanitize(promise.id))"
    },
    "human_readable_error": {
      "type": "string",
      "description": "= response.body.human_readable_error"
    },
    "report": {
      "type": "object",
      "description": "= response.body.report"
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
      "enum": ["table_not_found", "job_not_found", "invalid_request", "import_failed", "cancelled"]
    },
    "detail": {
      "description": "table_not_found / job_not_found / invalid_request: = response.body.detail of the 400/404; import_failed / cancelled: = response.body (the terminal job object)"
    }
  },
  "required": ["code"]
}
```

### 4.12.3 Integration Request

```
POST /api/database/tables/{promise.param.table_id}/import/async/ → 200
```

```json
{
  "type": "object",
  "properties": {
    "data": {
      "type": "array",
      "description": "= promise.param.data",
      "items": {
        "type": "array",
        "items": {}
      }
    },
    "configuration": {
      "type": "object",
      "description": "= promise.param.configuration"
    },
    "importer_type": {
      "type": "string",
      "description": "= promise.param.importer_type"
    },
    "original_file_name": {
      "type": "string",
      "description": "= sanitize(promise.id)"
    }
  },
  "required": [
    "data",
    "original_file_name"
  ],
  "additionalProperties": false
}
```

### 4.12.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "integer",
      "description": "Job id; the argument of job.get."
    },
    "type": {
      "type": "string",
      "description": "file_import for jobs this operation creates."
    },
    "state": {
      "type": "string",
      "enum": ["pending", "started", "finished", "failed", "cancelled"],
      "description": "finished, failed and cancelled are terminal; pending and started are running."
    },
    "progress_percentage": {
      "type": "integer",
      "description": "0 to 100."
    },
    "human_readable_error": {
      "type": "string",
      "description": "Empty unless the job failed."
    },
    "created_on": {
      "type": "string",
      "description": "ISO 8601."
    },
    "updated_on": {
      "type": "string",
      "description": "ISO 8601."
    },
    "database_id": {
      "type": "integer"
    },
    "table_id": {
      "type": "integer"
    },
    "name": {
      "type": "string"
    },
    "first_row_header": {
      "type": "boolean"
    },
    "importer_type": {
      "type": "string"
    },
    "original_file_name": {
      "type": "string",
      "description": "Echo of the submitted value; the stamp this operation recovers its job by."
    },
    "report": {
      "type": "object",
      "properties": {
        "failing_rows": {
          "type": "object",
          "description": "Keyed by the row's index in data; each value is an object of error messages by field name. Empty when every row was created.",
          "additionalProperties": true
        }
      },
      "required": ["failing_rows"]
    },
    "error": {
      "type": "string",
      "description": "Present on 4xx only. Machine readable failure code."
    },
    "detail": {
      "description": "Present on 4xx only. String or object."
    }
  },
  "required": ["id", "type", "state", "progress_percentage", "created_on", "updated_on", "database_id", "report"]
}
```

### 4.12.5 Implementation

| | |
|---|---|
| **Invocation** | `fetch_then_create` |
| **Monitoring** | `request_poll` |

```python
def _find_import_job(cfg, auth, stamp):
    # A file_import job echoes original_file_name, and GET /api/jobs/ lists
    # this account's jobs newest first, at most 100 per page, retained
    # BASEROW_JOB_EXPIRATION_TIME_LIMIT (30 days). Ten pages bound the scan;
    # a stamp older than the account's last 1000 file_import jobs is not
    # found and the import is repeated, duplicating its rows.
    offset = 0
    for _ in range(10):
        r = requests.get(
            f"{cfg.base_url}/api/jobs/",
            headers=auth,
            params={"type": "file_import", "limit": 100, "offset": offset},
            timeout=10,
        )
        _check(r)
        if r.status_code >= 400:
            raise Exception("release", r.text)
        jobs = r.json()["jobs"]
        for job in jobs:
            if job.get("original_file_name") == stamp:
                return job
        if len(jobs) < 100:
            return None
        offset += 100
    return None


def table_import(cfg, promise):
    auth = _token(cfg)
    args = promise.param["args"]
    table_id = quote(str(args["table_id"]), safe="")
    stamp = sanitize(promise.id)

    job = _find_import_job(cfg, auth, stamp)
    if job is None:
        body = {"data": args["data"], "original_file_name": stamp}
        for k in ("configuration", "importer_type"):
            if k in args:
                body[k] = args[k]
        r = requests.post(
            f"{cfg.base_url}/api/database/tables/{table_id}/import/async/",
            headers=auth,
            json=body,
            timeout=10,
        )
        if r.status_code == 404:
            return ("rejected", {"code": "table_not_found", "detail": _detail(r)})
        if r.status_code == 400 and _error(r) == "ERROR_MAX_JOB_COUNT_EXCEEDED":
            # Clears on its own once this account's running file_import jobs
            # finish.
            raise Exception("release", r.text)
        _check(r)
        if r.status_code >= 400:
            return ("rejected", {"code": "invalid_request", "detail": _detail(r)})
        job = r.json()

    job_id = quote(str(job["id"]), safe="")
    while job["state"] not in JOB_TERMINAL:
        if time.time() * 1000 >= promise.timeout_at:
            raise Exception("release", "promise timed out")
        time.sleep(
            min(
                cfg.poll_table.total_seconds(),
                (promise.timeout_at - time.time() * 1000) / 1000,
            )
        )
        g = requests.get(f"{cfg.base_url}/api/jobs/{job_id}/", headers=auth, timeout=10)
        if g.status_code == 401:
            auth = _token(cfg)
            continue
        if g.status_code == 404:
            return ("rejected", {"code": "job_not_found", "detail": _detail(g)})
        _check(g)
        if g.status_code >= 400:
            return ("rejected", {"code": "invalid_request", "detail": _detail(g)})
        job = g.json()

    if job["state"] == "finished":
        keys = (
            "id",
            "type",
            "state",
            "progress_percentage",
            "table_id",
            "database_id",
            "original_file_name",
            "human_readable_error",
            "report",
        )
        # A finished job can still carry report.failing_rows: those rows were
        # rejected by their field types and were not created.
        return ("resolved", {k: job[k] for k in keys})  # the 4.12.2 Resolved mapping
    if job["state"] == "cancelled":
        return ("rejected", {"code": "cancelled", "detail": job})
    return ("rejected", {"code": "import_failed", "detail": job})
```


### 4.13 job.get

| | |
|---|---|
| **Documentation** | [Get job](https://api.baserow.io/api/redoc/#tag/Jobs/operation/get_job) |

```json
{ "func": "job.get", "args": { ... } }
```

### 4.13.1 Promise Param Schema

```json
{
  "description": "Read one background job — its type, state, progress and, for a file_import, its report. Resolves with the job. Rejects job_not_found when no such job belongs to this account, and invalid_request on a malformed request. A plain read — not the completion mechanism; table.import observes independently.",
  "type": "object",
  "properties": {
    "job_id": {
      "type": "integer",
      "description": "Job id, as returned by table.import."
    }
  },
  "required": [
    "job_id"
  ],
  "additionalProperties": false
}
```

### 4.13.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "description": "= response.body"
}
```

#### Rejected

Same as 4.11.2.

### 4.13.3 Integration Request

```
GET /api/jobs/{promise.param.job_id}/ → 200
```

### 4.13.4 Integration Response

Same as 4.12.4.

### 4.13.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def job_get(cfg, promise):
    auth = _token(cfg)
    args = promise.param["args"]
    job_id = quote(str(args["job_id"]), safe="")

    r = requests.get(f"{cfg.base_url}/api/jobs/{job_id}/", headers=auth, timeout=10)
    if r.status_code == 404:
        return ("rejected", {"code": "job_not_found", "detail": _detail(r)})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": _detail(r)})
    return ("resolved", r.json())
```


### 4.14 database.list

| | |
|---|---|
| **Documentation** | [List all applications](https://api.baserow.io/api/redoc/#tag/Applications/operation/list_all_applications) |

```json
{ "func": "database.list", "args": { ... } }
```

### 4.14.1 Promise Param Schema

```json
{
  "description": "List every application these credentials can see, with the tables of each database inline. This is the entry read: it yields the database ids table.list takes and the table ids every row operation takes, without a workspace call first. Resolves with the array. Rejects invalid_request on a malformed request.",
  "type": "object",
  "properties": {},
  "additionalProperties": false
}
```

### 4.14.2 Promise Value Schema

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
      "description": "= response.body.detail of the 400"
    }
  },
  "required": ["code"]
}
```

### 4.14.3 Integration Request

```
GET /api/applications/ → 200
```

### 4.14.4 Integration Response

```json
{
  "type": "array",
  "items": {
    "type": "object",
    "properties": {
      "id": {
        "type": "integer",
        "description": "Application id. For type \"database\" this is the database_id table.list takes."
      },
      "name": {
        "type": "string"
      },
      "order": {
        "type": "integer"
      },
      "type": {
        "type": "string",
        "enum": ["database", "dashboard", "automation", "builder"],
        "description": "Only \"database\" applications carry tables."
      },
      "workspace": {
        "type": "object",
        "properties": {
          "id": {
            "type": "integer"
          },
          "name": {
            "type": "string"
          }
        },
        "additionalProperties": true
      },
      "created_on": {
        "type": "string",
        "description": "ISO 8601."
      },
      "tables": {
        "type": "array",
        "description": "Present on type \"database\".",
        "items": {
          "description": "Same as 4.15.4 items"
        }
      }
    },
    "required": ["id", "name", "order", "type", "workspace", "created_on"]
  }
}
```

### 4.14.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def database_list(cfg, promise):
    auth = _token(cfg)

    r = requests.get(f"{cfg.base_url}/api/applications/", headers=auth, timeout=10)
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": _detail(r)})
    return ("resolved", r.json())
```


### 4.15 table.list

| | |
|---|---|
| **Documentation** | [List tables](https://api.baserow.io/api/redoc/#tag/Database-tables/operation/list_database_tables) |

```json
{ "func": "table.list", "args": { ... } }
```

### 4.15.1 Promise Param Schema

```json
{
  "description": "List the tables of one database — the per-database narrowing of database.list, for instances whose application list is large. Resolves with the array. Rejects database_not_found when the database does not exist, and invalid_request on a malformed request.",
  "type": "object",
  "properties": {
    "database_id": {
      "type": "integer",
      "description": "Database to list. Enumerate via database.list."
    }
  },
  "required": [
    "database_id"
  ],
  "additionalProperties": false
}
```

### 4.15.2 Promise Value Schema

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
      "enum": ["database_not_found", "invalid_request"]
    },
    "detail": {
      "description": "= response.body.detail of the 400/404"
    }
  },
  "required": ["code"]
}
```

### 4.15.3 Integration Request

```
GET /api/database/tables/database/{promise.param.database_id}/ → 200
```

### 4.15.4 Integration Response

```json
{
  "type": "array",
  "items": {
    "type": "object",
    "properties": {
      "id": {
        "type": "integer",
        "description": "The table_id every row operation takes."
      },
      "name": {
        "type": "string"
      },
      "order": {
        "type": "integer",
        "description": "Lowest first."
      },
      "database_id": {
        "type": "integer"
      },
      "data_sync": {
        "type": ["object", "null"],
        "description": "Non-null when the table's rows are kept in step with an external source.",
        "additionalProperties": true
      }
    },
    "required": ["id", "name", "order", "database_id", "data_sync"]
  }
}
```

### 4.15.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def table_list(cfg, promise):
    auth = _token(cfg)
    args = promise.param["args"]
    database_id = quote(str(args["database_id"]), safe="")

    r = requests.get(
        f"{cfg.base_url}/api/database/tables/database/{database_id}/",
        headers=auth,
        timeout=10,
    )
    if r.status_code == 404:
        return ("rejected", {"code": "database_not_found", "detail": _detail(r)})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": _detail(r)})
    return ("resolved", r.json())
```


### 4.16 field.list

| | |
|---|---|
| **Documentation** | [List fields](https://api.baserow.io/api/redoc/#tag/Database-table-fields/operation/list_database_table_fields) |

```json
{ "func": "field.list", "args": { ... } }
```

### 4.16.1 Promise Param Schema

```json
{
  "description": "List the fields of one table. This is the read that constrains every write: it supplies each field's id, name and type, which decide the keys a row body may carry, the values each key accepts, the field ids export.create's fields and order_by take, and the order of the values in table.import's data. Resolves with the array. Rejects table_not_found when the table does not exist, and invalid_request on a malformed request.",
  "type": "object",
  "properties": {
    "table_id": {
      "type": "integer"
    }
  },
  "required": [
    "table_id"
  ],
  "additionalProperties": false
}
```

### 4.16.2 Promise Value Schema

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
      "enum": ["table_not_found", "invalid_request"]
    },
    "detail": {
      "description": "= response.body.detail of the 400/404"
    }
  },
  "required": ["code"]
}
```

### 4.16.3 Integration Request

```
GET /api/database/fields/table/{promise.param.table_id}/ → 200
```

### 4.16.4 Integration Response

```json
{
  "type": "array",
  "items": {
    "type": "object",
    "properties": {
      "id": {
        "type": "integer",
        "description": "The {id} of the field_{id} key in a row body."
      },
      "table_id": {
        "type": "integer"
      },
      "name": {
        "type": "string",
        "description": "The key in a row body when user_field_names is true."
      },
      "order": {
        "type": "integer",
        "description": "Lowest first."
      },
      "type": {
        "type": "string",
        "enum": ["text", "long_text", "url", "email", "number", "rating", "boolean", "date", "last_modified", "last_modified_by", "created_on", "created_by", "duration", "link_row", "file", "single_select", "multiple_select", "phone_number", "formula", "count", "rollup", "lookup", "multiple_collaborators", "uuid", "autonumber", "password", "ai", "data_sync"],
        "description": "Decides the shape of the value a row body may write to this field."
      },
      "primary": {
        "type": "boolean",
        "description": "The table's primary field. It cannot be deleted."
      },
      "read_only": {
        "type": "boolean",
        "description": "true when a row body cannot write this field."
      },
      "description": {
        "type": ["string", "null"]
      },
      "database_id": {
        "type": "integer"
      },
      "workspace_id": {
        "type": "integer"
      }
    },
    "required": ["id", "table_id", "name", "order", "type", "primary", "read_only"],
    "additionalProperties": true
  }
}
```

### 4.16.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def field_list(cfg, promise):
    auth = _token(cfg)
    args = promise.param["args"]
    table_id = quote(str(args["table_id"]), safe="")

    r = requests.get(
        f"{cfg.base_url}/api/database/fields/table/{table_id}/",
        headers=auth,
        timeout=10,
    )
    if r.status_code == 404:
        return ("rejected", {"code": "table_not_found", "detail": _detail(r)})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": _detail(r)})
    return ("resolved", r.json())
```


### 4.17 view.list

| | |
|---|---|
| **Documentation** | [List views](https://api.baserow.io/api/redoc/#tag/Database-table-views/operation/list_database_table_views) |

```json
{ "func": "view.list", "args": { ... } }
```

### 4.17.1 Promise Param Schema

```json
{
  "description": "List the views of one table. Supplies the view id that row.list's view_id, the row operations' view and export.create's view_id take. Resolves with the array. Rejects table_not_found when the table does not exist, and invalid_request on a malformed query.",
  "type": "object",
  "properties": {
    "table_id": {
      "type": "integer"
    },
    "type": {
      "type": "string",
      "description": "Return only views of this type, e.g. grid or gallery."
    },
    "limit": {
      "type": "integer",
      "minimum": 1,
      "description": "Return at most this many views. This endpoint has no pagination and no default limit."
    },
    "include": {
      "type": "array",
      "description": "Extra attributes to attach to each view: filters, sortings, decorations, group_bys, default_row_values. Joined with commas on the wire.",
      "items": {
        "type": "string",
        "enum": ["filters", "sortings", "decorations", "group_bys", "default_row_values"]
      }
    }
  },
  "required": [
    "table_id"
  ],
  "additionalProperties": false
}
```

### 4.17.2 Promise Value Schema

#### Resolved

```json
{
  "type": "array",
  "description": "= response.body"
}
```

#### Rejected

Same as 4.16.2.

### 4.17.3 Integration Request

```
GET /api/database/views/table/{promise.param.table_id}/{?type,limit,include = promise.param.*} → 200
```

### 4.17.4 Integration Response

```json
{
  "type": "array",
  "items": {
    "type": "object",
    "properties": {
      "id": {
        "type": "integer",
        "description": "The view id row.list, the row operations and export.create take."
      },
      "table_id": {
        "type": "integer"
      },
      "name": {
        "type": "string"
      },
      "order": {
        "type": "integer"
      },
      "type": {
        "type": "string",
        "description": "The view type, e.g. grid."
      },
      "filter_type": {
        "type": "string",
        "enum": ["AND", "OR"],
        "description": "How the view's own filters combine."
      },
      "filters_disabled": {
        "type": "boolean"
      },
      "ownership_type": {
        "type": "string"
      },
      "owned_by_id": {
        "type": ["integer", "null"]
      },
      "public": {
        "type": "boolean"
      },
      "slug": {
        "type": "string",
        "description": "Identifies the view on Baserow's public-view endpoints."
      }
    },
    "required": ["id", "table_id", "name", "order", "type"],
    "additionalProperties": true
  }
}
```

### 4.17.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def view_list(cfg, promise):
    auth = _token(cfg)
    args = promise.param["args"]
    table_id = quote(str(args["table_id"]), safe="")

    r = requests.get(
        f"{cfg.base_url}/api/database/views/table/{table_id}/",
        headers=auth,
        params=_query(args, ("type", "limit", "include")),
        timeout=10,
    )
    if r.status_code == 404:
        return ("rejected", {"code": "table_not_found", "detail": _detail(r)})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": _detail(r)})
    return ("resolved", r.json())
```


### 4.18 rowhistory.list

| | |
|---|---|
| **Documentation** | [List row change history](https://api.baserow.io/api/redoc/#tag/Database-table-rows/operation/get_database_table_row_history) |

```json
{ "func": "rowhistory.list", "args": { ... } }
```

### 4.18.1 Promise Param Schema

```json
{
  "description": "List one row's change history — per action, the fields it touched with their before and after values — the read that shows what a write actually changed. Resolves with the page. Rejects table_not_found or row_not_found when the named table or row does not exist, and invalid_request on a malformed query.",
  "type": "object",
  "properties": {
    "table_id": {
      "type": "integer"
    },
    "row_id": {
      "type": "integer"
    },
    "limit": {
      "type": "integer",
      "minimum": 1,
      "maximum": 200,
      "description": "Entries per page. Capped by BASEROW_ROW_PAGE_SIZE_LIMIT, 200 by default, which is also the value when omitted."
    },
    "offset": {
      "type": "integer",
      "minimum": 0,
      "description": "0 when omitted."
    }
  },
  "required": [
    "table_id",
    "row_id"
  ],
  "additionalProperties": false
}
```

### 4.18.2 Promise Value Schema

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
      "enum": ["table_not_found", "row_not_found", "invalid_request"]
    },
    "detail": {
      "description": "= response.body.detail of the 400/404"
    }
  },
  "required": ["code"]
}
```

### 4.18.3 Integration Request

```
GET /api/database/rows/table/{promise.param.table_id}/{promise.param.row_id}/history/{?limit,offset = promise.param.*} → 200
```

### 4.18.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "count": {
      "type": "integer"
    },
    "next": {
      "type": ["string", "null"]
    },
    "previous": {
      "type": ["string", "null"]
    },
    "results": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "id": {
            "type": "integer"
          },
          "action_type": {
            "type": "string",
            "description": "The action that changed the row, e.g. update_rows."
          },
          "action_command_type": {
            "type": "string",
            "enum": ["DO", "UNDO", "REDO"]
          },
          "user": {
            "type": "object",
            "properties": {
              "id": {
                "type": "integer"
              },
              "name": {
                "type": "string"
              }
            },
            "additionalProperties": true
          },
          "timestamp": {
            "type": "string",
            "description": "ISO 8601."
          },
          "before": {
            "type": "object",
            "description": "Values keyed by field_{id}, as they were before the action.",
            "additionalProperties": true
          },
          "after": {
            "type": "object",
            "description": "Values keyed by field_{id}, as they were after the action.",
            "additionalProperties": true
          },
          "fields_metadata": {
            "type": "object",
            "description": "Per changed field, its id and type.",
            "additionalProperties": true
          }
        },
        "required": ["id", "action_type", "user", "timestamp", "before", "after", "fields_metadata"]
      }
    },
    "error": {
      "type": "string",
      "description": "Present on 4xx only. Machine readable failure code."
    },
    "detail": {
      "description": "Present on 4xx only. String or object."
    }
  },
  "required": ["count", "next", "previous", "results"]
}
```

### 4.18.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def rowhistory_list(cfg, promise):
    auth = _token(cfg)
    args = promise.param["args"]
    table_id = quote(str(args["table_id"]), safe="")
    row_id = quote(str(args["row_id"]), safe="")

    r = requests.get(
        f"{cfg.base_url}/api/database/rows/table/{table_id}/{row_id}/history/",
        headers=auth,
        params=_query(args, ("limit", "offset")),
        timeout=10,
    )
    if r.status_code == 404:
        e = _error(r)
        if e == "ERROR_TABLE_DOES_NOT_EXIST":
            return ("rejected", {"code": "table_not_found", "detail": _detail(r)})
        if e == "ERROR_ROW_DOES_NOT_EXIST":
            return ("rejected", {"code": "row_not_found", "detail": _detail(r)})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": _detail(r)})
    return ("resolved", r.json())
```

## 5. Test

### 5.1 Base Image

```dockerfile
FROM baserow/baserow:2.3.3

# The entrypoint exits 1 unless /baserow/data is a mounted volume; this
# fixture keeps its data in the container.
ENV DISABLE_VOLUME_CHECK=yes
ENV BASEROW_PUBLIC_URL=http://localhost:8000

# Template synchronisation occupies the celery worker that runs export and
# file_import jobs for several minutes after the first migration, leaving
# both kinds of job at "pending" meanwhile.
ENV BASEROW_TRIGGER_SYNC_TEMPLATES_AFTER_MIGRATION=false
```

### 5.2 Run

```sh
docker rm -f plugin-baserow-test >/dev/null 2>&1 || true
docker build -t plugin-baserow-test spec/
docker run -d --name plugin-baserow-test -p 8000:80 plugin-baserow-test

BASEROW_BASE_URL=http://localhost:8000
BASEROW_EMAIL=resonate@example.com
BASEROW_PASSWORD=resonate-test-password

# The first boot runs migrations before the API answers; minting a token is
# the slowest warm-up path.
until curl -sf -o /dev/null -X POST "$BASEROW_BASE_URL/api/user/" -H 'Content-Type: application/json' \
  -d "{\"name\": \"Resonate\", \"email\": \"$BASEROW_EMAIL\", \"password\": \"$BASEROW_PASSWORD\"}"; do sleep 5; done
until TOKEN=$(curl -sf -X POST "$BASEROW_BASE_URL/api/user/token-auth/" -H 'Content-Type: application/json' \
  -d "{\"email\": \"$BASEROW_EMAIL\", \"password\": \"$BASEROW_PASSWORD\"}" \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['access_token'])") && [ -n "$TOKEN" ]; do sleep 5; done

AUTH="Authorization: JWT $TOKEN"
JSON="Content-Type: application/json"

# Registration creates no workspace in 2.3.3, so the fixture creates one
# before it can create an application.
WORKSPACE=$(curl -sf -X POST "$BASEROW_BASE_URL/api/workspaces/" -H "$AUTH" -H "$JSON" \
  -d '{"name": "Resonate"}' | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")
DATABASE=$(curl -sf -X POST "$BASEROW_BASE_URL/api/applications/workspace/$WORKSPACE/" -H "$AUTH" -H "$JSON" \
  -d '{"name": "Ops", "type": "database"}' | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")

# Every field of Tasks is text, so every value imports and every export
# finishes.
BASEROW_FIXTURE_OK=$(curl -sf -X POST "$BASEROW_BASE_URL/api/database/tables/database/$DATABASE/" -H "$AUTH" -H "$JSON" \
  -d '{"name": "Tasks", "first_row_header": true, "data": [["Name", "Notes"], ["Alpha", "first"], ["Beta", "second"]]}' \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")

# Amount is a number field, so importing a non-numeric value into it leaves
# the job at "finished" with that row named in report.failing_rows.
BASEROW_FIXTURE_FAIL=$(curl -sf -X POST "$BASEROW_BASE_URL/api/database/tables/database/$DATABASE/" -H "$AUTH" -H "$JSON" \
  -d '{"name": "Amounts", "first_row_header": true, "data": [["Label", "Amount"], ["a", "1"]]}' \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")
AMOUNT=$(curl -sf "$BASEROW_BASE_URL/api/database/fields/table/$BASEROW_FIXTURE_FAIL/" -H "$AUTH" \
  | python3 -c "import json,sys; print([f['id'] for f in json.load(sys.stdin) if f['name'] == 'Amount'][0])")
curl -sf -o /dev/null -X PATCH "$BASEROW_BASE_URL/api/database/fields/$AMOUNT/" -H "$AUTH" -H "$JSON" \
  -d '{"type": "number"}'

export BASEROW_BASE_URL
export BASEROW_EMAIL
export BASEROW_PASSWORD
export BASEROW_FIXTURE_OK
export BASEROW_FIXTURE_FAIL
```
