# Grafana

| | |
|---|---|
| **API** | `{base_url}/api` |
| **Idempotency** | No idempotency. Every operation is a read: `query.run` executes data source queries and returns their frames, `datasource.list` and `datasource.get` read configuration. No operation creates Grafana-side state and no value is injected into Grafana, so a re-delivered request repeats the read; a `query.run` query is forwarded to its data source unchanged, so whatever that statement does on the queried system is repeated with it. |
| **Reviewed by** | Claude Opus 5, 2026-08-29 |

**Notes**

| | |
|---|---|
| **OpenAPI** | `https://raw.githubusercontent.com/grafana/grafana/v13.2.0/public/api-merged.json` |
| **Self-hosted** | yes — §5 |

## 1. Address

```
grafana://[{instance}]           # omitted instance = "default"
```

## 2. Configuration

```toml
[grafana.{instance}]             # [grafana] = [grafana.default]
```

| key | type | default | example |
|---|---|---|---|
| `base_url` | `String` | | `https://grafana.acme.com` |
| `token` | `String` | | `glsa_…` |

## 3. Authentication & Authorization

| | |
|---|---|
| **Documentation** | [Authentication options for the HTTP API](https://grafana.com/docs/grafana/latest/developer-resources/api-reference/http-api/authentication/) |
| **Probe** | `GET /api/org` → `200` |

```
Authorization: Bearer {token}
```

## 4. Operations

### 4.1 query.run

| | |
|---|---|
| **Documentation** | [Query a data source](https://grafana.com/docs/grafana/latest/developer-resources/api-reference/http-api/api-legacy/data_source/#query-a-data-source) |

```json
{ "func": "query.run", "args": { ... } }
```

### 4.1.1 Promise Param Schema

```json
{
  "description": "Execute one or more data source queries over a time range and return their result frames. Resolves when every query returned without an error. Rejects query_failed when every failed query carries a status that is a 4xx other than 429 — the queried system refused the query itself, and the per-refId entries in detail carry error, errorSource and status. Any other failure status — 429, 5xx, or no status at all — is a condition of the moment, not of the query, and is retried instead of settled. Rejects not_found when a referenced data source, or the plugin backing it, does not exist. Rejects invalid_request when the body is malformed, carries no queries, a query names no data source, or a server-side expression fails to parse.",
  "type": "object",
  "properties": {
    "queries": {
      "type": "array",
      "minItems": 1,
      "description": "One or more queries. Beyond the properties below, each item carries the query fields of its own data source type and is forwarded to that data source unchanged.",
      "items": {
        "type": "object",
        "properties": {
          "refId": {
            "type": "string",
            "description": "Identifier of this query; keys its entry in the response. \"A\" when omitted."
          },
          "datasource": {
            "type": "object",
            "description": "The data source to query.",
            "properties": {
              "uid": {
                "type": "string",
                "description": "Data source UID. Enumerate via datasource.list."
              },
              "type": {
                "type": "string",
                "description": "Data source plugin id, as reported by datasource.get .type."
              }
            },
            "required": [
              "uid"
            ],
            "additionalProperties": true
          },
          "format": {
            "type": "string",
            "enum": [
              "time_series",
              "table"
            ],
            "description": "Shape the data is returned in; which values a data source honours depends on the data source."
          },
          "maxDataPoints": {
            "type": "integer",
            "description": "Maximum number of data points the caller can render. 100 when omitted."
          },
          "intervalMs": {
            "type": "integer",
            "description": "Time series interval in milliseconds. 1000 when omitted."
          }
        },
        "required": [
          "datasource"
        ],
        "additionalProperties": true
      }
    },
    "from": {
      "type": "string",
      "description": "Start of the time range: epoch milliseconds, or a relative Grafana time unit expression such as \"now-5m\". The API declares it required; Grafana does not enforce it and runs a body without a range over an epoch-0 range."
    },
    "to": {
      "type": "string",
      "description": "End of the time range: epoch milliseconds, or a relative Grafana time unit expression such as \"now\". Declared required and unenforced, as for \"from\"."
    },
    "debug": {
      "type": "boolean",
      "description": "Debug flag carried on the request; the API declares it and documents no effect."
    }
  },
  "required": [
    "queries",
    "from",
    "to"
  ]
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
      "enum": ["not_found", "invalid_request", "query_failed"]
    },
    "detail": {
      "description": "not_found / invalid_request on the documented 404/400: = response.body of the error envelope; invalid_request on any other 4xx: the response body as text; query_failed: = response.body.results"
    }
  },
  "required": ["code"]
}
```

### 4.1.3 Integration Request

```
POST /api/ds/query → 200
```

```json
{
  "type": "object",
  "properties": {
    "queries": {
      "type": "array",
      "description": "= promise.param.queries"
    },
    "from": {
      "type": "string",
      "description": "= promise.param.from"
    },
    "to": {
      "type": "string",
      "description": "= promise.param.to"
    },
    "debug": {
      "type": "boolean",
      "description": "= promise.param.debug"
    }
  },
  "required": [
    "queries",
    "from",
    "to"
  ]
}
```

### 4.1.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "results": {
      "type": "object",
      "description": "Present on 200 and on a 400 whose cause is unsuccessful queries. One entry per query, keyed by that query's refId. The endpoint answers 200 or 400 and nothing else: the 207 the OpenAPI declares is emitted only by the alternative /apis/datasource.grafana.app routing, which sits behind the queryServiceRewrite feature toggle, experimental and false in this version.",
      "additionalProperties": {
        "type": "object",
        "properties": {
          "status": {
            "type": "integer",
            "description": "HTTP-shaped status the data source reported for this query; 200 on success. On failure it is the status of the failure: the status the queried system itself answered when it refused the query (400 for a query it rejected), 502 when the data source could not be reached at all, 500 when the plugin raised the error."
          },
          "frames": {
            "type": "array",
            "description": "Result frames in Grafana's data frame JSON: each item is {\"schema\": {\"refId\", \"meta\", \"fields\"}, \"data\": {\"values\"}}. Frames and an error coexist: a failed query's entry carries anything from no frames, through one frame with empty fields, to fully populated data.",
            "items": {
              "type": "object"
            }
          },
          "error": {
            "type": "string",
            "description": "Present only when this query failed."
          },
          "errorSource": {
            "type": "string",
            "enum": ["plugin", "downstream"],
            "description": "Present only when this query failed. plugin = the data source plugin itself; downstream = the system the plugin queried."
          }
        }
      }
    },
    "message": {
      "type": "string",
      "description": "Present instead of results on 400, 403, 404 and 500."
    },
    "error": {
      "type": "string",
      "description": "Part of Grafana's error envelope on 500: a short label, e.g. \"Server Error\"."
    },
    "title": {
      "type": "string",
      "description": "Part of Grafana's error envelope on 403: \"Access denied\"."
    },
    "accessErrorId": {
      "type": "string",
      "description": "Part of Grafana's error envelope on 403: an identifier for the denied access, e.g. \"ACE2204158796\"."
    },
    "messageId": {
      "type": "string",
      "description": "Part of Grafana's error envelope, present on some conditions: an identifier for the error, e.g. \"query.noQueries\", \"query.invalidDatasourceId\", \"sse.parseError\"."
    },
    "statusCode": {
      "type": "integer",
      "description": "Part of Grafana's error envelope, present on some conditions: repeats the HTTP status."
    },
    "extra": {
      "type": ["object", "null"],
      "description": "Part of Grafana's error envelope, present on some conditions: condition-specific detail.",
      "additionalProperties": true
    },
    "traceID": {
      "type": "string"
    }
  }
}
```

### 4.1.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
from urllib.parse import quote

import requests


def _check(r):
    # 401 unauthorisedError and 403 forbiddenError are the only statuses
    # Grafana documents for a rejected or under-permissioned credential;
    # on /api/ds/query a 403 is "Access denied" to a queried data source.
    if r.status_code in (401, 403):
        raise Exception("halt", r.text)
    if r.status_code == 429 or r.status_code >= 500:
        raise Exception("release", r.text)
    return r


def _auth(cfg):
    return {"Authorization": f"Bearer {cfg.token}"}


def query_run(cfg, promise):
    args = promise.param["args"]
    body = {k: args[k] for k in ("queries", "from", "to", "debug") if k in args}

    # [dataproxy] timeout — 30s by default — bounds only core backend HTTP
    # data sources; a plugin data source can run well past it, so a query
    # slow enough to outrun this read timeout raises instead of answering.
    r = _check(
        requests.post(
            f"{cfg.base_url}/api/ds/query",
            headers=_auth(cfg),
            json=body,
            timeout=60,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()})
    if r.status_code == 400:
        # 400 is both a malformed request and "one or more data source
        # queries were unsuccessful"; only the latter carries results.
        results = r.json().get("results")
        if results is None:
            return ("rejected", {"code": "invalid_request", "detail": r.json()})
        for entry in results.values():
            if entry.get("error") is None:
                continue
            # 400 carries the status the queried system answered when it
            # refused this query; 502 means it was never reached and 500
            # that the plugin raised, neither a fact about the query.
            status = entry.get("status")
            if not isinstance(status, int) or status == 429 or not 400 <= status < 500:
                raise Exception("release", r.text)
        return ("rejected", {"code": "query_failed", "detail": results})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json())
```

### 4.2 datasource.list

| | |
|---|---|
| **Documentation** | [Get all data sources](https://grafana.com/docs/grafana/latest/developer-resources/api-reference/http-api/api-legacy/data_source/#get-all-data-sources) |

```json
{ "func": "datasource.list", "args": { ... } }
```

### 4.2.1 Promise Param Schema

```json
{
  "description": "List the data sources of the token's organization — the uid and type each query.run query names. Takes no arguments and is not paginated: Grafana returns every data source up to [datasources] datasource_limit, 5000 by default.",
  "type": "object",
  "properties": {},
  "additionalProperties": false
}
```

### 4.2.2 Promise Value Schema

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
      "description": "invalid_request: the response body as text"
    }
  },
  "required": ["code"]
}
```

### 4.2.3 Integration Request

```
GET /api/datasources → 200
```

### 4.2.4 Integration Response

```json
{
  "type": "array",
  "items": {
    "type": "object",
    "properties": {
      "id": {
        "type": "integer",
        "description": "Numeric identity."
      },
      "uid": {
        "type": "string",
        "description": "External identity; the value a query.run query's datasource.uid carries."
      },
      "orgId": {
        "type": "integer"
      },
      "name": {
        "type": "string",
        "description": "Display name."
      },
      "type": {
        "type": "string",
        "description": "Data source plugin id, e.g. \"prometheus\", \"grafana-testdata-datasource\"."
      },
      "typeName": {
        "type": "string",
        "description": "Human-readable name of the plugin."
      },
      "typeLogoUrl": {
        "type": "string"
      },
      "access": {
        "type": "string",
        "enum": ["proxy", "direct"],
        "description": "\"proxy\" routes queries through the Grafana server."
      },
      "url": {
        "type": "string",
        "description": "Address of the queried system; empty for data sources that have none."
      },
      "user": {
        "type": "string"
      },
      "database": {
        "type": "string"
      },
      "basicAuth": {
        "type": "boolean"
      },
      "isDefault": {
        "type": "boolean",
        "description": "true for the organization's default data source."
      },
      "jsonData": {
        "type": "object",
        "description": "Plugin-specific, non-secret settings.",
        "additionalProperties": true
      },
      "readOnly": {
        "type": "boolean",
        "description": "true for a data source created by file provisioning."
      }
    }
  }
}
```

### 4.2.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def datasource_list(cfg, promise):
    r = _check(
        requests.get(
            f"{cfg.base_url}/api/datasources",
            headers=_auth(cfg),
            timeout=10,
        )
    )
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json())
```

### 4.3 datasource.get

| | |
|---|---|
| **Documentation** | [Get a single data source by uid](https://grafana.com/docs/grafana/latest/developer-resources/api-reference/http-api/api-legacy/data_source/#get-a-single-data-source-by-uid) |

```json
{ "func": "datasource.get", "args": { ... } }
```

### 4.3.1 Promise Param Schema

```json
{
  "description": "Fetch one data source by UID, including \"type\" and \"jsonData\" — the plugin id and settings that determine which query fields a query.run query for this data source must carry. Rejects not_found when no data source in the token's organization has this UID.",
  "type": "object",
  "properties": {
    "uid": {
      "type": "string",
      "description": "Data source UID. Enumerate via datasource.list."
    }
  },
  "required": [
    "uid"
  ],
  "additionalProperties": false
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
      "enum": ["not_found", "invalid_request"]
    },
    "detail": {
      "description": "not_found: = response.body of the 404 error envelope; invalid_request: the response body as text"
    }
  },
  "required": ["code"]
}
```

### 4.3.3 Integration Request

```
GET /api/datasources/uid/{promise.param.uid} → 200
```

### 4.3.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "integer",
      "description": "Same as 4.2.4 .[].id"
    },
    "uid": {
      "type": "string",
      "description": "Same as 4.2.4 .[].uid"
    },
    "orgId": {
      "type": "integer"
    },
    "name": {
      "type": "string"
    },
    "type": {
      "type": "string",
      "description": "Same as 4.2.4 .[].type"
    },
    "typeLogoUrl": {
      "type": "string"
    },
    "access": {
      "type": "string",
      "enum": ["proxy", "direct"]
    },
    "url": {
      "type": "string"
    },
    "user": {
      "type": "string"
    },
    "database": {
      "type": "string"
    },
    "basicAuth": {
      "type": "boolean"
    },
    "basicAuthUser": {
      "type": "string"
    },
    "withCredentials": {
      "type": "boolean"
    },
    "isDefault": {
      "type": "boolean"
    },
    "jsonData": {
      "type": "object",
      "description": "Same as 4.2.4 .[].jsonData",
      "additionalProperties": true
    },
    "secureJsonFields": {
      "type": "object",
      "description": "One entry per configured secret, name to true; the secrets themselves are never returned.",
      "additionalProperties": {
        "type": "boolean"
      }
    },
    "version": {
      "type": "integer",
      "description": "Version of the stored data source record."
    },
    "readOnly": {
      "type": "boolean",
      "description": "Same as 4.2.4 .[].readOnly"
    },
    "apiVersion": {
      "type": "string",
      "description": "Plugin API version of the data source; empty when the plugin declares none."
    },
    "accessControl": {
      "type": "object",
      "description": "The requesting identity's permissions on this data source, action to true; present only when access control metadata was requested.",
      "additionalProperties": {
        "type": "boolean"
      }
    },
    "message": {
      "type": "string",
      "description": "Present instead of the record on 400, 403, 404 and 500."
    }
  }
}
```

### 4.3.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def datasource_get(cfg, promise):
    uid = quote(promise.param["args"]["uid"], safe="")

    r = _check(
        requests.get(
            f"{cfg.base_url}/api/datasources/uid/{uid}",
            headers=_auth(cfg),
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json())
```

## 5. Test

### 5.1 Base Image

```dockerfile
FROM grafana/grafana:13.2.0-ubuntu
USER root
RUN apt-get update \
 && apt-get install -y --no-install-recommends python3 python3-requests \
 && rm -rf /var/lib/apt/lists/*
RUN printf '%s\n' \
  'import http.server, json' \
  'BODY = json.dumps({"status": "error", "errorType": "bad_data", "error": "this query is always refused"}).encode()' \
  'class H(http.server.BaseHTTPRequestHandler):' \
  '    def do_GET(self):' \
  '        self.send_response(400)' \
  '        self.send_header("Content-Type", "application/json")' \
  '        self.send_header("Content-Length", str(len(BODY)))' \
  '        self.end_headers()' \
  '        self.wfile.write(BODY)' \
  '    do_POST = do_GET' \
  '    def log_message(self, *args):' \
  '        pass' \
  'http.server.HTTPServer(("127.0.0.1", 9099), H).serve_forever()' \
  > /usr/local/bin/refuse.py
RUN printf '%s\n' \
  '#!/bin/sh' \
  'python3 /usr/local/bin/refuse.py &' \
  'exec /run.sh "$@"' \
  > /usr/local/bin/entrypoint.sh \
 && chmod +x /usr/local/bin/entrypoint.sh
RUN mkdir -p /etc/grafana/provisioning/datasources && printf '%s\n' \
  'apiVersion: 1' \
  'datasources:' \
  '  - name: FixtureOk' \
  '    uid: fixture-ok' \
  '    type: grafana-testdata-datasource' \
  '    access: proxy' \
  '    isDefault: true' \
  '  - name: FixtureFail' \
  '    uid: fixture-fail' \
  '    type: prometheus' \
  '    access: proxy' \
  '    url: http://127.0.0.1:9099' \
  > /etc/grafana/provisioning/datasources/fixtures.yaml
USER grafana
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
```

### 5.2 Run

```sh
docker rm -f plugin-grafana-test >/dev/null 2>&1 || true
docker build -t plugin-grafana-test spec/
docker run -d --name plugin-grafana-test -p 3000:3000 plugin-grafana-test

until curl -sf http://localhost:3000/api/health >/dev/null; do sleep 2; done
until curl -sf -u admin:admin http://localhost:3000/api/datasources/uid/fixture-ok >/dev/null; do sleep 2; done

SA_ID=$(curl -sf -u admin:admin -X POST http://localhost:3000/api/serviceaccounts \
  -H "Content-Type: application/json" -d '{"name": "resonate", "role": "Admin"}' \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")
GRAFANA_TOKEN=$(curl -sf -u admin:admin -X POST "http://localhost:3000/api/serviceaccounts/$SA_ID/tokens" \
  -H "Content-Type: application/json" -d '{"name": "resonate"}' \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['key'])")
until curl -sf -H "Authorization: Bearer $GRAFANA_TOKEN" http://localhost:3000/api/org >/dev/null; do sleep 2; done

export GRAFANA_BASE_URL=http://localhost:3000
export GRAFANA_TOKEN
export GRAFANA_FIXTURE_OK=fixture-ok
export GRAFANA_FIXTURE_FAIL=fixture-fail
```
