# Baserow

| | |
|---|---|
| **API** | `{base_url}/api` |
| **Idempotency** | No idempotency — `POST /api/database/export/table/{table_id}/` accepts no client-supplied key or id, the `ExportJob` record has no client-writable identity field, and no endpoint looks a job up by anything but its server-assigned integer id, so `sanitize(promise.id)` cannot be injected anywhere. Creating an export job additionally sets every other unfinished export job of the same user to state `cancelled` |
| **Reviewed by** | Claude Opus, 2026-08-27 |

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

## 3. Authentication & Authorization

| | |
|---|---|
| **Documentation** | [REST API](https://baserow.io/docs/apis%2Frest-api) |
| **Probe** | `GET /api/applications/` → `200` |

```
POST /api/user/token-auth/  {"email": "{email}", "password": "{password}"}  → 200 {"access_token": …, "refresh_token": …}
Authorization: JWT {access_token}
```

## 4. Operations

### 4.1 export.create

| | |
|---|---|
| **Documentation** | [Export table](https://api.baserow.io/api/redoc/#tag/Database-table-export/operation/export_table) |

```json
{ "func": "export.create", "args": { ... } }
```

### 4.1.1 Promise Param Schema

```json
{
  "description": "Export one table, or one grid view of it, to a file and observe the export job to a terminal state. Resolves when the job reaches state \"finished\", carrying the download url of the exported file. Rejects table_not_found if the table does not exist or is trashed, view_not_found if view_id does not exist, invalid_request if the options fail validation (unknown exporter_type, view not in the table, view type unsupported by the exporter, unknown field id in filters or order_by, filter type incompatible with the field type) or the credentials are not a member of the table's workspace, job_not_found if the job record disappears while polling, export_failed on state \"failed\", cancelled on state \"cancelled\", and expired on state \"expired\". Creating an export job cancels every other unfinished export job of the same user, so two concurrent baserow promises sharing one set of credentials will cancel each other, and a re-delivery leaves the previous job cancelled and exports again. Duration is the table's own export time — seconds for thousands of rows, minutes for very large tables; the file is deleted and the job set to \"expired\" EXPORT_FILE_EXPIRE_MINUTES (60 by default) after creation, so size timeoutAt below that.",
  "type": "object",
  "properties": {
    "table_id": {
      "type": "integer",
      "description": "Table to export. Enumerate via table.list."
    },
    "exporter_type": {
      "type": "string",
      "enum": ["csv", "json", "xml", "excel", "file"],
      "description": "File type. csv is always registered; json, xml, excel and file are registered by the premium module and require an active premium license for the table's workspace, otherwise the request answers 402."
    },
    "view_id": {
      "type": ["integer", "null"],
      "minimum": 0,
      "description": "Grid view of this table to export, using its filters, sorts and field options. null or omitted exports the whole table. Every registered exporter supports grid views only."
    },
    "export_charset": {
      "type": "string",
      "enum": [
        "utf-8", "iso-8859-6", "windows-1256", "iso-8859-4", "windows-1257",
        "iso-8859-14", "iso-8859-2", "windows-1250", "gbk", "gb18030", "big5",
        "koi8-r", "koi8-u", "iso-8859-5", "windows-1251", "x-mac-cyrillic",
        "iso-8859-7", "windows-1253", "iso-8859-8", "windows-1255", "euc-jp",
        "iso-2022-jp", "shift-jis", "euc-kr", "macintosh", "iso-8859-10",
        "iso-8859-16", "windows-874", "windows-1254", "windows-1258",
        "iso-8859-1", "windows-1252", "iso-8859-3"
      ],
      "description": "Character set of the exported file. \"utf-8\" when omitted."
    },
    "filters": {
      "type": ["object", "null"],
      "description": "Ad hoc filter tree applied to the exported rows, in addition to the saved filters of view_id.",
      "properties": {
        "filter_type": {
          "type": "string",
          "enum": ["AND", "OR"]
        },
        "filters": {
          "type": "array",
          "items": {
            "type": "object",
            "properties": {
              "field": {
                "type": "integer",
                "description": "Field id. Enumerate via field.list."
              },
              "type": {
                "type": "string",
                "description": "View filter type, e.g. equal, not_equal, contains, contains_not, higher_than, date_is_equal, single_select_equal, link_row_has, boolean, empty, not_empty. Must be compatible with the field's type."
              },
              "value": {
                "type": "string"
              }
            },
            "required": ["field", "type", "value"]
          }
        },
        "groups": {
          "type": "array",
          "items": {
            "description": "Same as 4.1.1 .filters"
          }
        }
      },
      "required": ["filter_type"]
    },
    "order_by": {
      "type": ["string", "null"],
      "description": "Comma separated field ids the rows are ordered by; ascending unless the id is prefixed with \"-\"."
    },
    "fields": {
      "type": ["array", "null"],
      "items": {
        "type": "integer"
      },
      "description": "Field ids to include, in the desired order. All fields when omitted."
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
      "description": "exporter_type \"csv\" only. \",\" when omitted."
    },
    "csv_include_header": {
      "type": "boolean",
      "description": "exporter_type \"csv\" only. true when omitted."
    },
    "excel_include_header": {
      "type": "boolean",
      "description": "exporter_type \"excel\" only. true when omitted."
    },
    "organize_files": {
      "type": "boolean",
      "description": "exporter_type \"file\" only; groups the exported files by row id inside the zip. true when omitted."
    }
  },
  "required": ["table_id", "exporter_type"]
}
```

### 4.1.2 Promise Value Schema

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
      "enum": [
        "table_not_found",
        "view_not_found",
        "invalid_request",
        "job_not_found",
        "export_failed",
        "cancelled",
        "expired"
      ]
    },
    "detail": {
      "description": "table_not_found / view_not_found / invalid_request: = response.body of the 400/404 ({error, detail}); export_failed / cancelled / expired: = response.body (the terminal export job object); job_not_found: = response.body of the 404 as text"
    }
  },
  "required": ["code"]
}
```

### 4.1.3 Integration Request

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
      "items": {
        "type": "integer"
      },
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
  "required": ["exporter_type"]
}
```

### 4.1.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "integer",
      "description": "External identity of the export job."
    },
    "table": {
      "type": ["integer", "null"],
      "description": "Id of the exported table; null once the table is deleted."
    },
    "view": {
      "type": ["integer", "null"],
      "description": "Id of the exported view; null for a table export."
    },
    "exporter_type": {
      "type": "string"
    },
    "state": {
      "type": "string",
      "enum": ["pending", "exporting", "finished", "failed", "cancelled", "expired"],
      "description": "finished, failed, cancelled and expired are terminal. pending and exporting are the running states; a running job is set to cancelled when the same user starts another export job, and to expired by the cleanup task EXPORT_FILE_EXPIRE_MINUTES after creation."
    },
    "status": {
      "type": "string",
      "description": "Deprecated; the value of state."
    },
    "exported_file_name": {
      "type": ["string", "null"],
      "description": "Storage file name; null while the job is pending, set when the export starts, before the file is complete, and null again once it is cleaned up."
    },
    "created_at": {
      "type": "string",
      "description": "ISO 8601"
    },
    "progress_percentage": {
      "type": "number",
      "description": "0.0 to 100.0."
    },
    "url": {
      "type": ["string", "null"],
      "description": "Download url of the exported file; null while exported_file_name is null."
    }
  },
  "required": ["id", "table", "view", "exporter_type", "state", "status", "exported_file_name", "created_at", "progress_percentage", "url"]
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

TERMINAL = ("finished", "failed", "cancelled", "expired")


def _check(r):
    # 401 PERMISSION_DENIED / ERROR_INVALID_ACCESS_TOKEN, 402
    # ERROR_FEATURE_NOT_AVAILABLE (no premium license for the workspace) and
    # 403 ERROR_FEATURE_DISABLED all end only when an operator acts.
    if r.status_code in (401, 402, 403):
        raise Exception("halt", r.text)
    if r.status_code == 429 or r.status_code >= 500:
        raise Exception("release", r.text)
    return r


def _auth(cfg):
    r = requests.post(
        f"{cfg.base_url}/api/user/token-auth/",
        json={"email": cfg.email, "password": cfg.password},
        timeout=10,
    )
    _check(r)
    if r.status_code >= 400:
        raise Exception("halt", r.text)
    body = r.json()
    if "access_token" not in body:
        # A 200 carrying two_factor_auth instead of the tokens: the account has
        # two-factor authentication enabled and cannot be authenticated with an
        # email and password alone.
        raise Exception("halt", r.text)
    return {"Authorization": f"JWT {body['access_token']}"}


def export_create(cfg, promise):
    api = f"{cfg.base_url}/api"
    auth = _auth(cfg)
    args = promise.param["args"]
    table_id = quote(str(args["table_id"]), safe="")
    body = {k: v for k, v in args.items() if k != "table_id"}

    # Creating an export job sets every other unfinished export job of this user
    # to cancelled, so a re-delivery costs the previous attempt's job and a
    # second full export of the table.
    r = requests.post(
        f"{api}/database/export/table/{table_id}/",
        headers=auth,
        json=body,
        timeout=10,
    )
    if r.status_code == 404:
        if r.json().get("error") == "ERROR_VIEW_DOES_NOT_EXIST":
            return ("rejected", {"code": "view_not_found", "detail": r.json()})
        return ("rejected", {"code": "table_not_found", "detail": r.json()})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()})

    job = r.json()
    failures = 0
    while job["state"] not in TERMINAL:
        if time.time() * 1000 >= promise.timeout_at:
            raise Exception("release", "promise timed out")
        time.sleep(min(cfg.poll.total_seconds(),
                       (promise.timeout_at - time.time() * 1000) / 1000))
        # The job id exists only in this attempt's memory: nothing on the job
        # record ties it to the promise, so a raise here loses the running job.
        try:
            r = requests.get(f"{api}/database/export/{job['id']}/", headers=auth, timeout=10)
            if r.status_code == 401:
                # Access tokens live BASEROW_ACCESS_TOKEN_LIFETIME_MINUTES
                # (10 by default); this endpoint runs no permission check, so a
                # 401 here is an expired token.
                auth = _auth(cfg)
                continue
            if r.status_code == 404:
                return ("rejected", {"code": "job_not_found", "detail": r.text})
            _check(r)
            job = r.json()
            failures = 0
        except Exception as e:
            if e.args[:1] == ("halt",):
                raise
            failures += 1
            if failures >= 5:
                raise

    if job["state"] == "finished":
        keys = (
            "id",
            "table",
            "view",
            "exporter_type",
            "state",
            "exported_file_name",
            "created_at",
            "url",
        )
        return ("resolved", {k: job[k] for k in keys})  # the 4.1.2 Resolved mapping
    if job["state"] == "cancelled":
        return ("rejected", {"code": "cancelled", "detail": job})
    if job["state"] == "expired":
        return ("rejected", {"code": "expired", "detail": job})
    return ("rejected", {"code": "export_failed", "detail": job})
```

### 4.2 export.get

| | |
|---|---|
| **Documentation** | [Get export job](https://api.baserow.io/api/redoc/#tag/Database-table-export/operation/get_export_job) |

```json
{ "func": "export.get", "args": { ... } }
```

### 4.2.1 Promise Param Schema

```json
{
  "description": "Read one export job of the authenticated user. Rejects not_found if no export job with this id belongs to the credentials. A plain read — not the completion mechanism; export.create observes independently.",
  "type": "object",
  "properties": {
    "job_id": {
      "type": "integer",
      "description": "Export job id."
    }
  },
  "required": ["job_id"]
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
      "enum": ["not_found", "invalid_request"]
    },
    "detail": {
      "description": "not_found: = response.body of the 404 as text; invalid_request: = response.body of the 400 as text"
    }
  },
  "required": ["code"]
}
```

### 4.2.3 Integration Request

```
GET /api/database/export/{promise.param.job_id}/ → 200
```

### 4.2.4 Integration Response

Same as 4.1.4.

### 4.2.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def export_get(cfg, promise):
    api = f"{cfg.base_url}/api"
    auth = _auth(cfg)
    job_id = quote(str(promise.param["args"]["job_id"]), safe="")

    r = requests.get(f"{api}/database/export/{job_id}/", headers=auth, timeout=10)
    # ERROR_EXPORT_JOB_DOES_NOT_EXIST also answers for a job of another user.
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.text})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json())
```

### 4.3 table.list

| | |
|---|---|
| **Documentation** | [List database tables](https://api.baserow.io/api/redoc/#tag/Database-tables/operation/list_database_tables) |

```json
{ "func": "table.list", "args": { ... } }
```

### 4.3.1 Promise Param Schema

```json
{
  "description": "List the tables of one database — the table ids export.create takes. Rejects not_found if the database does not exist or is trashed, invalid_request if the credentials are not a member of the database's workspace.",
  "type": "object",
  "properties": {
    "database_id": {
      "type": "integer",
      "description": "Database application id, as shown in the url of the Baserow interface and by GET /api/applications/."
    }
  },
  "required": ["database_id"]
}
```

### 4.3.2 Promise Value Schema

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
      "enum": ["not_found", "invalid_request"]
    },
    "detail": {
      "description": "not_found: = response.body of the 404 as text; invalid_request: = response.body of the 400 as text"
    }
  },
  "required": ["code"]
}
```

### 4.3.3 Integration Request

```
GET /api/database/tables/database/{promise.param.database_id}/ → 200
```

### 4.3.4 Integration Response

```json
{
  "type": "array",
  "items": {
    "type": "object",
    "properties": {
      "id": {
        "type": "integer"
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
        "description": "Always present; null for a table not backed by a data sync.",
        "properties": {
          "id": {
            "type": "integer"
          },
          "type": {
            "type": "string"
          },
          "table_id": {
            "type": "integer"
          },
          "database_id": {
            "type": ["integer", "null"]
          },
          "synced_properties": {
            "type": "array"
          },
          "last_sync": {
            "type": ["string", "null"],
            "description": "ISO 8601"
          },
          "last_error": {
            "type": ["string", "null"]
          },
          "auto_add_new_properties": {
            "type": "boolean"
          },
          "delete_unmatched_rows": {
            "type": "boolean"
          },
          "two_way_sync": {
            "type": "boolean"
          }
        }
      }
    },
    "required": ["id", "name", "order", "database_id"]
  }
}
```

### 4.3.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def table_list(cfg, promise):
    api = f"{cfg.base_url}/api"
    auth = _auth(cfg)
    database_id = quote(str(promise.param["args"]["database_id"]), safe="")

    r = requests.get(
        f"{api}/database/tables/database/{database_id}/", headers=auth, timeout=10
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.text})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json())
```

### 4.4 field.list

| | |
|---|---|
| **Documentation** | [List database table fields](https://api.baserow.io/api/redoc/#tag/Database-table-fields/operation/list_database_table_fields) |

```json
{ "func": "field.list", "args": { ... } }
```

### 4.4.1 Promise Param Schema

```json
{
  "description": "List the fields of one table — the field ids export.create takes in fields, order_by and filters. Rejects table_not_found if the table does not exist or is trashed, view_not_found if view does not exist in that table, invalid_request if the credentials are not a member of the table's workspace.",
  "type": "object",
  "properties": {
    "table_id": {
      "type": "integer",
      "description": "Table whose fields are listed."
    },
    "view": {
      "type": "integer",
      "description": "Restrict to the fields visible in this view of the table."
    }
  },
  "required": ["table_id"]
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
      "enum": ["table_not_found", "view_not_found", "invalid_request"]
    },
    "detail": {
      "description": "table_not_found / view_not_found: = response.body of the 404 ({error, detail}); invalid_request: = response.body of the 400 as text"
    }
  },
  "required": ["code"]
}
```

### 4.4.3 Integration Request

```
GET /api/database/fields/table/{promise.param.table_id}/{?view = promise.param.*} → 200
```

### 4.4.4 Integration Response

```json
{
  "type": "array",
  "items": {
    "type": "object",
    "description": "Properties beyond these depend on the field's type.",
    "additionalProperties": true,
    "properties": {
      "id": {
        "type": "integer"
      },
      "table_id": {
        "type": "integer"
      },
      "name": {
        "type": "string"
      },
      "order": {
        "type": "integer",
        "description": "Lowest first."
      },
      "type": {
        "type": "string",
        "description": "Field type, e.g. text, long_text, number, boolean, date, single_select, multiple_select, link_row, formula, file."
      },
      "primary": {
        "type": "boolean",
        "description": "One field per table is primary."
      },
      "read_only": {
        "type": "boolean"
      },
      "immutable_type": {
        "type": "boolean"
      },
      "immutable_properties": {
        "type": "boolean"
      },
      "description": {
        "type": ["string", "null"]
      },
      "database_id": {
        "type": "integer"
      },
      "workspace_id": {
        "type": "integer"
      },
      "db_index": {
        "type": "boolean"
      },
      "field_constraints": {
        "type": "array"
      }
    },
    "required": ["id", "table_id", "name", "order", "type", "primary", "read_only"]
  }
}
```

### 4.4.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def field_list(cfg, promise):
    api = f"{cfg.base_url}/api"
    auth = _auth(cfg)
    args = promise.param["args"]
    table_id = quote(str(args["table_id"]), safe="")
    params = {k: args[k] for k in ("view",) if k in args}

    r = requests.get(
        f"{api}/database/fields/table/{table_id}/",
        headers=auth,
        params=params,
        timeout=10,
    )
    if r.status_code == 404:
        if r.json().get("error") == "ERROR_VIEW_DOES_NOT_EXIST":
            return ("rejected", {"code": "view_not_found", "detail": r.json()})
        return ("rejected", {"code": "table_not_found", "detail": r.json()})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json())
```

## 5. Test

### 5.1 Base Image

```dockerfile
FROM baserow/baserow:2.3.3
ENV BASEROW_PUBLIC_URL=http://localhost:8000
```

### 5.2 Run

```sh
set -e

docker rm -f plugin-baserow-test >/dev/null 2>&1 || true
docker build -t plugin-baserow-test spec/
docker run -d --name plugin-baserow-test -p 8000:80 plugin-baserow-test

BASEROW_BASE_URL=http://localhost:8000
BASEROW_EMAIL=resonate@example.com
BASEROW_PASSWORD=resonate-test-password-1

until curl -sf "$BASEROW_BASE_URL/api/_health/" >/dev/null; do sleep 5; done

# Registration with no template_id creates the user and one workspace and
# nothing else — no database and no table. Retry until a token can be minted:
# the web server answers before the backend and its celery worker are ready.
until TOKEN=$(curl -sf -X POST "$BASEROW_BASE_URL/api/user/token-auth/" \
  -H "Content-Type: application/json" \
  -d "{\"email\": \"$BASEROW_EMAIL\", \"password\": \"$BASEROW_PASSWORD\"}" \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['access_token'])" 2>/dev/null) \
  && [ -n "$TOKEN" ]; do
  curl -s -X POST "$BASEROW_BASE_URL/api/user/" -H "Content-Type: application/json" \
    -d "{\"name\": \"Resonate\", \"email\": \"$BASEROW_EMAIL\", \"password\": \"$BASEROW_PASSWORD\", \"authenticate\": true}" >/dev/null
  sleep 5
done

WORKSPACE_ID=$(curl -sf "$BASEROW_BASE_URL/api/workspaces/" -H "Authorization: JWT $TOKEN" \
  | python3 -c "import json,sys; print(json.load(sys.stdin)[0]['id'])")
DATABASE_ID=$(curl -sf -X POST "$BASEROW_BASE_URL/api/applications/workspace/$WORKSPACE_ID/" \
  -H "Authorization: JWT $TOKEN" -H "Content-Type: application/json" \
  -d '{"name": "Resonate", "type": "database"}' \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")

# Omitting data seeds the new table with example fields and rows, so the
# exported file is not empty.
FIXTURE_OK=$(curl -sf -X POST "$BASEROW_BASE_URL/api/database/tables/database/$DATABASE_ID/" \
  -H "Authorization: JWT $TOKEN" -H "Content-Type: application/json" \
  -d '{"name": "Fixture OK"}' \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")
FIXTURE_FAIL=$(curl -sf -X POST "$BASEROW_BASE_URL/api/database/tables/database/$DATABASE_ID/" \
  -H "Authorization: JWT $TOKEN" -H "Content-Type: application/json" \
  -d '{"name": "Fixture Fail"}' \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")

# Trashing the second table makes its id permanently unresolvable: TableHandler
# excludes trashed tables, so exporting it answers 404 and the rejection is
# table_not_found at create time — no ExportJob is created. Baserow documents no
# feature that drives an export job to state "failed", so export_failed has no
# fixture; "cancelled" needs none (start a second export for the same user while
# one is unfinished) and "expired" occurs only 60 minutes after creation.
curl -sf -X DELETE "$BASEROW_BASE_URL/api/database/tables/$FIXTURE_FAIL/" -H "Authorization: JWT $TOKEN"

export BASEROW_BASE_URL
export BASEROW_EMAIL
export BASEROW_PASSWORD
export BASEROW_FIXTURE_OK=$FIXTURE_OK
export BASEROW_FIXTURE_FAIL=$FIXTURE_FAIL
```
