# Elasticsearch

| | |
|---|---|
| **API** | `{base_url}` |
| **Idempotency** | No client-supplied key: `POST /_reindex` accepts no reindex id and the task id is assigned by the cluster. Reindex tasks are locatable by request header: `sanitize(promise.id)` is injected as `X-Opaque-Id`, which Elasticsearch stores verbatim in the task's `headers` and returns from `GET /_tasks` for as long as the task runs. The header takes an arbitrary value (the full 117-character `sanitize` yield round-trips) and carries no uniqueness constraint — Elasticsearch never deduplicates on it. A completed task leaves the task listing, so the recovery window is the reindex's own runtime |
| **Reviewed by** | Claude Opus, 2026-08-29 |

**Notes**

| | |
|---|---|
| **OpenAPI** | `https://raw.githubusercontent.com/elastic/elasticsearch-specification/main/output/openapi/elasticsearch-openapi.json` |
| **Self-hosted** | yes — §5 |

## 1. Address

```
elasticsearch://[{instance}]     # omitted instance = "default"
```

## 2. Configuration

```toml
[elasticsearch.{instance}]       # [elasticsearch] = [elasticsearch.default]
```

| key | type | default | example |
|---|---|---|---|
| `base_url` | `String` | | `https://es.acme.com:9200` |
| `api_key` | `String` | | `…` |
| `poll` | `Duration` | `2s` | `2s` |

## 3. Authentication & Authorization

| | |
|---|---|
| **Documentation** | [Token-based authentication services](https://www.elastic.co/docs/deploy-manage/users-roles/cluster-or-deployment-auth/token-based-authentication-services#token-authentication-api-key) |
| **Probe** | `GET /_security/_authenticate` → `200` |

```
Authorization: ApiKey {api_key}
```

## 4. Operations

### 4.1 reindex.create

| | |
|---|---|
| **Documentation** | [Reindex documents](https://www.elastic.co/docs/api/doc/elasticsearch/operation/operation-reindex) |

```json
{ "func": "reindex.create", "args": { ... } }
```

### 4.1.1 Promise Param Schema

```json
{
  "description": "Copy documents from a source data stream, index or alias into a destination and observe the reindex task to a terminal state. Resolves when the task completes with no error, no failures and no cancellation. Rejects invalid_request when Elasticsearch refuses the submitted request (absent source or dest, an unknown field nested inside source, dest or script), reindex_failed when the task completes carrying an error (unknown source index, source == dest, a document whose index does not store _source, an unsupported script language, an action the API key is not authorized for) or completes with a non-empty failures array (version conflicts while conflicts is \"abort\", per-document rejections by the destination), and cancelled when the task is cancelled through the task management API. Duration is the copy's own runtime — seconds for a small index, hours for a large one; size timeoutAt accordingly. A redelivery that arrives after the task has completed no longer finds it in the task listing and copies a second time: with dest.op_type \"index\" the same document ids are overwritten, with \"create\" the second copy ends in version conflicts.",
  "type": "object",
  "properties": {
    "source": {
      "type": "object",
      "properties": {
        "index": {
          "type": ["string", "array"],
          "items": {
            "type": "string"
          },
          "description": "Data stream, index or alias to copy from; a list copies from several. Enumerate via index.list."
        },
        "query": {
          "type": "object",
          "description": "Query DSL selecting the documents to copy. All documents when omitted.",
          "additionalProperties": true
        },
        "remote": {
          "type": "object",
          "description": "A remote Elasticsearch cluster to copy from. The host must be allowed by the destination cluster's reindex.remote.whitelist node setting.",
          "properties": {
            "host": {
              "type": "string",
              "description": "Scheme, host and port of the remote cluster."
            },
            "username": {
              "type": "string",
              "description": "Required with password for basic auth against the remote host."
            },
            "password": {
              "type": "string"
            },
            "api_key": {
              "type": "string",
              "description": "Alternative to basic auth. Not permitted together with an Authorization entry in headers."
            },
            "headers": {
              "type": "object",
              "additionalProperties": {
                "type": "string"
              }
            },
            "connect_timeout": {
              "type": "string",
              "description": "Duration; \"30s\" when omitted."
            },
            "socket_timeout": {
              "type": "string",
              "description": "Duration; \"30s\" when omitted."
            }
          },
          "required": [
            "host"
          ],
          "additionalProperties": false
        },
        "size": {
          "type": "number",
          "description": "Documents per batch; 1000 when omitted."
        },
        "slice": {
          "type": "object",
          "description": "Manual slicing of this request. Not supported when reindexing from a remote cluster.",
          "properties": {
            "id": {
              "type": "number"
            },
            "max": {
              "type": "number"
            },
            "field": {
              "type": "string"
            }
          },
          "required": [
            "id",
            "max"
          ],
          "additionalProperties": false
        },
        "_source": {
          "type": ["boolean", "array"],
          "items": {
            "type": "string"
          },
          "description": "true reindexes all source fields; a list reindexes the named fields. true when omitted."
        },
        "runtime_mappings": {
          "type": "object",
          "description": "Runtime fields defined for the source search.",
          "additionalProperties": true
        }
      },
      "required": [
        "index"
      ],
      "additionalProperties": false
    },
    "dest": {
      "type": "object",
      "properties": {
        "index": {
          "type": "string",
          "description": "Data stream, index or index alias to copy into. Must differ from the source. Created from the matching index template when it does not exist — settings, shard counts and mappings are not copied from the source."
        },
        "op_type": {
          "type": "string",
          "enum": ["index", "create"],
          "description": "\"index\" overwrites documents with the same id; \"create\" indexes only missing documents and turns every existing id into a version conflict. A data stream destination requires \"create\". \"index\" when omitted."
        },
        "pipeline": {
          "type": "string",
          "description": "Ingest pipeline to run on each document."
        },
        "routing": {
          "type": "string",
          "description": "\"keep\" preserves the source routing, \"discard\" clears it, \"=value\" sets it to value. \"keep\" when omitted. Not allowed when index.slice.enabled is true on the destination."
        },
        "version_type": {
          "type": "string",
          "enum": ["internal", "external", "external_gte"],
          "description": "\"internal\" overwrites blindly; \"external\" preserves the source version and updates only older destination documents."
        }
      },
      "required": [
        "index"
      ],
      "additionalProperties": false
    },
    "conflicts": {
      "type": "string",
      "enum": ["abort", "proceed"],
      "description": "\"abort\" ends the reindex on the first version conflict and reports it in failures; \"proceed\" counts conflicts and continues. \"abort\" when omitted."
    },
    "max_docs": {
      "type": "number",
      "description": "Maximum documents to reindex. All documents when omitted. Split evenly across slices when slices is set."
    },
    "script": {
      "type": "object",
      "description": "Script updating each document's source or metadata.",
      "properties": {
        "source": {
          "type": "string"
        },
        "id": {
          "type": "string",
          "description": "Id of a stored script; alternative to source."
        },
        "lang": {
          "type": "string",
          "enum": ["painless", "expression", "mustache", "java"],
          "description": "\"painless\" when omitted."
        },
        "params": {
          "type": "object",
          "additionalProperties": true
        },
        "options": {
          "type": "object",
          "additionalProperties": {
            "type": "string"
          }
        }
      },
      "additionalProperties": false
    },
    "refresh": {
      "type": "boolean",
      "description": "Refresh the affected shards when the copy finishes. false when omitted."
    },
    "requests_per_second": {
      "type": "number",
      "description": "Throttle across the whole reindex, slices included; -1 turns throttling off. -1 when omitted."
    },
    "slices": {
      "oneOf": [
        {
          "type": "number",
          "minimum": 1
        },
        {
          "type": "string",
          "enum": ["auto"]
        }
      ],
      "description": "Subtasks to divide the reindex into; \"auto\" is one slice per shard up to a limit. 1 when omitted. Not supported when reindexing from a remote cluster."
    },
    "timeout": {
      "type": "string",
      "description": "Duration each indexing operation waits for automatic index creation, dynamic mapping updates and active shards. \"1m\" when omitted."
    },
    "wait_for_active_shards": {
      "oneOf": [
        {
          "type": "number"
        },
        {
          "type": "string",
          "enum": ["all", "index-setting"]
        }
      ],
      "description": "Shard copies that must be active before the copy proceeds. 1 when omitted."
    },
    "require_alias": {
      "type": "boolean",
      "description": "Require the destination to be an index alias. false when omitted."
    }
  },
  "required": [
    "source",
    "dest"
  ],
  "additionalProperties": false
}
```

### 4.1.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "string",
      "description": "= response.body.id"
    },
    "description": {
      "type": "string",
      "description": "= response.body.description"
    },
    "start_time_in_millis": {
      "type": "number",
      "description": "= response.body.start_time_in_millis"
    },
    "running_time_in_nanos": {
      "type": "number",
      "description": "= response.body.running_time_in_nanos"
    },
    "cancelled": {
      "type": "boolean",
      "description": "= response.body.cancelled"
    },
    "response": {
      "type": "object",
      "description": "= response.body.response"
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
      "enum": ["invalid_request", "reindex_failed", "cancelled"]
    },
    "detail": {
      "description": "invalid_request: = response.body.error of the 400; reindex_failed: = response.body.error (the terminal task error) or = response.body.response.failures (the failures that ended the copy); cancelled: = response.body.response.canceled, absent when the task record reports cancellation only through cancelled"
    }
  },
  "required": ["code"]
}
```

### 4.1.3 Integration Request

```
POST /_reindex?wait_for_completion=false{?refresh,requests_per_second,slices,timeout,wait_for_active_shards,require_alias = promise.param.*} → 200
X-Opaque-Id: {sanitize(promise.id)}
```

```json
{
  "type": "object",
  "properties": {
    "source": {
      "type": "object",
      "description": "= promise.param.source"
    },
    "dest": {
      "type": "object",
      "description": "= promise.param.dest"
    },
    "conflicts": {
      "type": "string",
      "description": "= promise.param.conflicts"
    },
    "max_docs": {
      "type": "number",
      "description": "= promise.param.max_docs"
    },
    "script": {
      "type": "object",
      "description": "= promise.param.script"
    }
  },
  "required": [
    "source",
    "dest"
  ],
  "additionalProperties": false
}
```

### 4.1.4 Integration Response

```json
{
  "type": "object",
  "description": "The reindex task record, the body of GET /_reindex/{task_id}. POST /_reindex?wait_for_completion=false returns {\"task\": <id>} only — the id of this record.",
  "properties": {
    "completed": {
      "type": "boolean",
      "description": "true is terminal: the task has stopped and the record is its stored result."
    },
    "id": {
      "type": "string",
      "description": "External identity, of the form <node id>:<task number>. Assigned when the task was first created and stable across node-shutdown relocations."
    },
    "description": {
      "type": "string",
      "description": "Sanitized source and destination of the copy, e.g. \"reindex from [src] to [dst]\"."
    },
    "start_time_in_millis": {
      "type": "number",
      "description": "Milliseconds since the Unix epoch."
    },
    "running_time_in_nanos": {
      "type": "number"
    },
    "cancelled": {
      "type": "boolean",
      "description": "Whether the task has been cancelled. A cancellation that lands while the copy is finishing is reported only by response.canceled."
    },
    "status": {
      "type": "object",
      "description": "Progress of the copy while it runs; present whether or not the task is completed.",
      "properties": {
        "total": {
          "type": "number"
        },
        "created": {
          "type": "number"
        },
        "updated": {
          "type": "number"
        },
        "deleted": {
          "type": "number"
        },
        "batches": {
          "type": "number"
        },
        "version_conflicts": {
          "type": "number"
        },
        "noops": {
          "type": "number"
        },
        "retries": {
          "type": "object",
          "properties": {
            "bulk": {
              "type": "number"
            },
            "search": {
              "type": "number"
            }
          }
        },
        "requests_per_second": {
          "type": "number"
        },
        "throttled_millis": {
          "type": "number"
        },
        "throttled": {
          "type": "string",
          "description": "throttled_millis as a duration string, e.g. \"0s\"."
        },
        "throttled_until_millis": {
          "type": "number"
        },
        "throttled_until": {
          "type": "string",
          "description": "throttled_until_millis as a duration string, e.g. \"0s\"."
        },
        "canceled": {
          "type": "string",
          "description": "Reason for cancellation; present only on a cancelled task."
        },
        "slices": {
          "type": "array",
          "description": "Per-slice status of a sliced reindex; null entries are slices that have not started.",
          "items": {
            "type": ["object", "null"],
            "description": "The same counters as this object, plus slice_id, the slice's position in the sliced request."
          }
        }
      }
    },
    "error": {
      "type": "object",
      "description": "The error that ended the task; present only on a completed task that failed, and never together with response.",
      "properties": {
        "type": {
          "type": "string",
          "description": "Exception type, e.g. index_not_found_exception, illegal_argument_exception, action_request_validation_exception, security_exception."
        },
        "reason": {
          "type": ["string", "null"]
        },
        "caused_by": {
          "type": "object"
        },
        "root_cause": {
          "type": "array"
        }
      },
      "required": [
        "type"
      ]
    },
    "response": {
      "type": "object",
      "description": "The final result; present only on a completed task that ran to its end.",
      "properties": {
        "took": {
          "type": "number",
          "description": "Milliseconds the whole copy took."
        },
        "timed_out": {
          "type": "boolean",
          "description": "true if any request run during the copy timed out."
        },
        "total": {
          "type": "number"
        },
        "created": {
          "type": "number"
        },
        "updated": {
          "type": "number"
        },
        "deleted": {
          "type": "number"
        },
        "batches": {
          "type": "number"
        },
        "version_conflicts": {
          "type": "number"
        },
        "noops": {
          "type": "number"
        },
        "retries": {
          "type": "object",
          "properties": {
            "bulk": {
              "type": "number"
            },
            "search": {
              "type": "number"
            }
          }
        },
        "requests_per_second": {
          "type": "number"
        },
        "throttled_millis": {
          "type": "number"
        },
        "throttled_until_millis": {
          "type": "number",
          "description": "Always 0 in a completed result."
        },
        "throttled": {
          "type": "string",
          "description": "throttled_millis as a duration string, e.g. \"0s\"."
        },
        "throttled_until": {
          "type": "string",
          "description": "throttled_until_millis as a duration string, always \"0s\" in a completed result."
        },
        "canceled": {
          "type": "string",
          "description": "Reason for cancellation; present only when the copy was cancelled."
        },
        "slices": {
          "type": "array",
          "description": "Per-slice results of a sliced reindex.",
          "items": {
            "type": "object",
            "description": "The same counters as this object, plus slice_id, the slice's position in the sliced request."
          }
        },
        "failures": {
          "type": "array",
          "description": "Unrecoverable failures. A non-empty array means the copy ended because of them; version conflicts appear here unless conflicts is \"proceed\".",
          "items": {
            "type": "object",
            "properties": {
              "index": {
                "type": "string"
              },
              "id": {
                "type": "string"
              },
              "status": {
                "type": "number"
              },
              "cause": {
                "type": "object",
                "properties": {
                  "type": {
                    "type": "string"
                  },
                  "reason": {
                    "type": ["string", "null"]
                  }
                },
                "required": [
                  "type"
                ]
              }
            },
            "required": [
              "index",
              "id",
              "status",
              "cause"
            ]
          }
        }
      }
    }
  },
  "required": [
    "completed",
    "id",
    "start_time_in_millis",
    "running_time_in_nanos",
    "cancelled"
  ]
}
```

### 4.1.5 Implementation

| | |
|---|---|
| **Invocation** | `fetch_then_create` |
| **Monitoring** | `request_poll` |

```python
import time
from urllib.parse import quote

import requests

REINDEX_ACTION = "indices:data/write/reindex"


def _check(r):
    # 401: the API key is rejected. 403: security_exception — the key lacks a
    # cluster or index privilege the action requires.
    if r.status_code in (401, 403):
        raise Exception("halt", r.text)
    if r.status_code == 429 or r.status_code >= 500:
        raise Exception("release", r.text)
    return r


def _auth(cfg):
    return {"Authorization": f"ApiKey {cfg.api_key}"}


def _params(args, keys):
    # Elasticsearch parses only lowercase "true"/"false", and reads a query key
    # repeated across occurrences as its last occurrence — a list of values is
    # sent comma-separated.
    params = {}
    for k in keys:
        if k in args:
            v = args[k]
            if isinstance(v, bool):
                v = "true" if v else "false"
            elif isinstance(v, list):
                v = ",".join(v)
            params[k] = v
    return params


def _running(cfg, token):
    # A running reindex still carries X-Opaque-Id in its task headers. A sliced
    # reindex repeats the header on its child tasks, which carry parent_task_id.
    # A node that stops responding mid-listing is reported in node_failures and
    # its tasks are missing from this response.
    r = _check(requests.get(
        f"{cfg.base_url}/_tasks",
        headers=_auth(cfg),
        params={"actions": REINDEX_ACTION},
        timeout=10,
    ))
    for node in r.json().get("nodes", {}).values():
        for task_id, task in node["tasks"].items():
            if task.get("headers", {}).get("X-Opaque-Id") == token and "parent_task_id" not in task:
                return task_id
    return None


def reindex_create(cfg, promise):
    args = promise.param["args"]
    token = sanitize(promise.id)

    task_id = _running(cfg, token)
    if task_id is None:
        body = {k: args[k] for k in ("source", "dest", "conflicts", "max_docs", "script") if k in args}
        params = _params(args, (
            "refresh", "requests_per_second", "slices", "timeout",
            "wait_for_active_shards", "require_alias",
        ))
        params["wait_for_completion"] = "false"
        # A completed task has left the task listing: a copy that already
        # finished is copied again, overwriting the same document ids under
        # op_type "index" and conflicting under "create".
        r = requests.post(
            f"{cfg.base_url}/_reindex",
            headers=_auth(cfg) | {"X-Opaque-Id": token},
            params=params,
            json=body,
            timeout=10,
        )
        _check(r)
        if r.status_code >= 400:
            # Residue: identical bytes every redelivery — permanent. Only the
            # request itself is validated here; an unknown source index, a bad
            # script or an unauthorized action end the task instead.
            return ("rejected", {"code": "invalid_request", "detail": r.json()["error"]})
        task_id = r.json()["task"]

    task = quote(task_id, safe="")
    failed = 0
    while True:
        if time.time() * 1000 >= promise.timeout_at:
            raise Exception("release", "promise timed out")
        try:
            r = requests.get(f"{cfg.base_url}/_reindex/{task}", headers=_auth(cfg), timeout=10)
            if r.status_code == 404:
                # The id no longer resolves: the task's node left the cluster
                # with no stored result. Redelivery re-enters through the task
                # listing.
                raise Exception("release", r.text)
            _check(r)
            run = r.json()
        except Exception as e:
            # A completed task has left the task listing, so leaving the loop
            # costs a second full copy of the whole index.
            if e.args[:1] == ("halt",) or failed >= 4:
                raise
            failed += 1
            time.sleep(min(cfg.poll.total_seconds(),
                           (promise.timeout_at - time.time() * 1000) / 1000))
            continue
        failed = 0
        if run["completed"]:
            break
        time.sleep(min(cfg.poll.total_seconds(),
                       (promise.timeout_at - time.time() * 1000) / 1000))

    if "error" in run:
        return ("rejected", {"code": "reindex_failed", "detail": run["error"]})
    result = run.get("response", {})
    if run["cancelled"] or "canceled" in result:
        return ("rejected", {"code": "cancelled", "detail": result.get("canceled")})
    if result.get("failures"):
        return ("rejected", {"code": "reindex_failed", "detail": result["failures"]})
    keys = ("id", "description", "start_time_in_millis", "running_time_in_nanos",
            "cancelled", "response")
    return ("resolved", {k: run[k] for k in keys if k in run})  # the 4.1.2 Resolved mapping
```

### 4.2 reindex.get

| | |
|---|---|
| **Documentation** | [Get the status and progress of a specific reindex task](https://www.elastic.co/docs/api/doc/elasticsearch/operation/operation-get-reindex) |

```json
{ "func": "reindex.get", "args": { ... } }
```

### 4.2.1 Promise Param Schema

```json
{
  "description": "Read one reindex task record — its progress while it runs, its error or result once it has completed. Rejects not_found if the id is unknown, names a task that is not a reindex, names a sliced child subtask, or names a task whose node left the cluster with no stored result, invalid_request if the id is not of the form <node id>:<task number>. A plain read — not the completion mechanism; reindex.create observes independently.",
  "type": "object",
  "properties": {
    "task_id": {
      "type": "string",
      "description": "Reindex task id, e.g. \"r1A2WoRbTwKZ516z6NEs5A:36619\". A relocated task is followed transparently and answers under the original id."
    }
  },
  "required": [
    "task_id"
  ]
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
      "description": "invalid_request: = response.body.error of the 400; not_found: absent"
    }
  },
  "required": ["code"]
}
```

### 4.2.3 Integration Request

```
GET /_reindex/{promise.param.task_id} → 200
```

### 4.2.4 Integration Response

Same as 4.1.4.

### 4.2.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def reindex_get(cfg, promise):
    task_id = quote(promise.param["args"]["task_id"], safe="")

    r = requests.get(f"{cfg.base_url}/_reindex/{task_id}", headers=_auth(cfg), timeout=10)
    if r.status_code == 404:
        return ("rejected", {"code": "not_found"})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["error"]})
    return ("resolved", r.json())
```

### 4.3 index.list

| | |
|---|---|
| **Documentation** | [Resolve indices](https://www.elastic.co/docs/api/doc/elasticsearch/operation/operation-indices-resolve-index) |

```json
{ "func": "index.list", "args": { ... } }
```

### 4.3.1 Promise Param Schema

```json
{
  "description": "Resolve a name or index pattern into the indices, aliases and data streams it matches, for discovering the names reindex.create copies from and into. Rejects not_found if a concrete, non-wildcarded target does not exist, invalid_request on a malformed parameter.",
  "type": "object",
  "properties": {
    "name": {
      "type": "string",
      "description": "Comma-separated names or index patterns. A target on a remote cluster is written <cluster>:<name>."
    },
    "expand_wildcards": {
      "type": ["string", "array"],
      "items": {
        "type": "string",
        "enum": ["all", "open", "closed", "hidden", "none"]
      },
      "description": "Index states a wildcard may match; comma-separated values are allowed. \"open\" when omitted."
    },
    "ignore_unavailable": {
      "type": "boolean",
      "description": "true silently ignores a missing, closed or otherwise unavailable concrete target. false when omitted."
    },
    "allow_no_indices": {
      "type": "boolean",
      "description": "false makes a wildcard that matches nothing, or an overall empty resolution, an error. true when omitted."
    },
    "mode": {
      "type": ["string", "array"],
      "items": {
        "type": "string",
        "enum": ["standard", "time_series", "logsdb", "lookup"]
      },
      "description": "Restrict to these index modes. No filter when omitted."
    }
  },
  "required": [
    "name"
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
      "enum": ["not_found", "invalid_request"]
    },
    "detail": {
      "description": "= response.body.error of the 404/400"
    }
  },
  "required": ["code"]
}
```

### 4.3.3 Integration Request

```
GET /_resolve/index/{promise.param.name}{?expand_wildcards,ignore_unavailable,allow_no_indices,mode = promise.param.*} → 200
```

### 4.3.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "indices": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "name": {
            "type": "string",
            "description": "Concrete index name."
          },
          "aliases": {
            "type": "array",
            "items": {
              "type": "string"
            }
          },
          "attributes": {
            "type": "array",
            "description": "State of the index, e.g. \"open\", \"closed\", \"hidden\", \"system\". A closed index cannot be reindexed from.",
            "items": {
              "type": "string"
            }
          },
          "data_stream": {
            "type": "string",
            "description": "Data stream this index backs."
          },
          "mode": {
            "type": "string",
            "enum": ["standard", "time_series", "logsdb", "lookup"]
          }
        },
        "required": [
          "name",
          "attributes"
        ]
      }
    },
    "aliases": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "name": {
            "type": "string"
          },
          "indices": {
            "type": "array",
            "items": {
              "type": "string"
            }
          }
        },
        "required": [
          "name",
          "indices"
        ]
      }
    },
    "data_streams": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "name": {
            "type": "string"
          },
          "timestamp_field": {
            "type": "string"
          },
          "backing_indices": {
            "type": "array",
            "items": {
              "type": "string"
            }
          }
        },
        "required": [
          "name",
          "timestamp_field",
          "backing_indices"
        ]
      }
    }
  },
  "required": [
    "indices",
    "aliases",
    "data_streams"
  ]
}
```

### 4.3.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def index_list(cfg, promise):
    args = promise.param["args"]

    params = _params(args, (
        "expand_wildcards", "ignore_unavailable", "allow_no_indices", "mode",
    ))
    name = quote(args["name"], safe="")
    r = requests.get(
        f"{cfg.base_url}/_resolve/index/{name}",
        headers=_auth(cfg),
        params=params,
        timeout=10,
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["error"]})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["error"]})
    return ("resolved", r.json())
```

### 4.4 index.get

| | |
|---|---|
| **Documentation** | [Get index information](https://www.elastic.co/docs/api/doc/elasticsearch/operation/operation-indices-get) |

```json
{ "func": "index.get", "args": { ... } }
```

### 4.4.1 Promise Param Schema

```json
{
  "description": "Fetch one index with its aliases, mappings and settings — the shape a reindex.create destination must already have, since reindex copies neither settings nor mappings from the source. Rejects not_found if a concrete target does not exist, invalid_request on a malformed parameter.",
  "type": "object",
  "properties": {
    "index": {
      "type": "string",
      "description": "Comma-separated data streams, indices and aliases; wildcards are supported. For a data stream, its backing indices are returned."
    },
    "allow_no_indices": {
      "type": "boolean",
      "description": "false makes a wildcard that matches nothing, or an overall empty resolution, an error. true when omitted."
    },
    "expand_wildcards": {
      "type": ["string", "array"],
      "items": {
        "type": "string",
        "enum": ["all", "open", "closed", "hidden", "none"]
      },
      "description": "Index states a wildcard may match. \"open\" when omitted."
    },
    "features": {
      "type": ["string", "array"],
      "items": {
        "type": "string",
        "enum": ["aliases", "mappings", "settings"]
      },
      "description": "Sections to return. Validated against the enum, but without effect on the response: aliases, mappings and settings are all returned whatever the value."
    },
    "flat_settings": {
      "type": "boolean",
      "description": "Return settings in flat form. false when omitted."
    },
    "ignore_unavailable": {
      "type": "boolean",
      "description": "true silently ignores a missing, closed or otherwise unavailable concrete target. false when omitted."
    },
    "include_defaults": {
      "type": "boolean",
      "description": "Include all default settings. false when omitted."
    },
    "local": {
      "type": "boolean",
      "description": "Answer from the local node instead of the master. false when omitted."
    },
    "master_timeout": {
      "type": "string",
      "description": "Duration to wait for a connection to the master node. \"30s\" when omitted."
    }
  },
  "required": [
    "index"
  ]
}
```

### 4.4.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "description": "= response.body"
}
```

#### Rejected

Same as 4.3.2.

### 4.4.3 Integration Request

```
GET /{promise.param.index}{?allow_no_indices,expand_wildcards,features,flat_settings,ignore_unavailable,include_defaults,local,master_timeout = promise.param.*} → 200
```

### 4.4.4 Integration Response

```json
{
  "type": "object",
  "description": "One property per matched concrete index, keyed by index name.",
  "additionalProperties": {
    "type": "object",
    "properties": {
      "aliases": {
        "type": "object",
        "description": "Alias names of this index, each mapped to its alias definition.",
        "additionalProperties": true
      },
      "mappings": {
        "type": "object",
        "description": "Field mappings of this index.",
        "additionalProperties": true
      },
      "settings": {
        "type": "object",
        "description": "Index settings, nested under \"index\" unless flat_settings is set.",
        "additionalProperties": true
      },
      "defaults": {
        "type": "object",
        "description": "Default settings; present only when include_defaults is set.",
        "additionalProperties": true
      },
      "data_stream": {
        "type": "string",
        "description": "Data stream this index backs."
      },
      "lifecycle": {
        "type": "object",
        "description": "Data stream lifecycle of this index.",
        "additionalProperties": true
      }
    }
  }
}
```

### 4.4.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def index_get(cfg, promise):
    args = promise.param["args"]

    params = _params(args, (
        "allow_no_indices", "expand_wildcards", "features", "flat_settings",
        "ignore_unavailable", "include_defaults", "local", "master_timeout",
    ))
    index = quote(args["index"], safe="")
    r = requests.get(
        f"{cfg.base_url}/{index}",
        headers=_auth(cfg),
        params=params,
        timeout=10,
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["error"]})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["error"]})
    return ("resolved", r.json())
```

## 5. Test

### 5.1 Base Image

```dockerfile
FROM elasticsearch:9.5.2

USER root
RUN microdnf install -y python3 python3-pip \
 && microdnf clean all \
 && python3 -m pip install --no-cache-dir requests
USER elasticsearch

ENV discovery.type=single-node \
    xpack.security.enabled=true \
    xpack.security.http.ssl.enabled=false \
    xpack.license.self_generated.type=basic \
    cluster.routing.allocation.disk.threshold_enabled=false \
    ELASTIC_PASSWORD=resonate \
    ES_JAVA_OPTS=-Xms1g\ -Xmx1g
```

### 5.2 Run

```sh
docker rm -f plugin-elasticsearch-test >/dev/null 2>&1 || true
docker build -t plugin-elasticsearch-test spec/
docker run -d --name plugin-elasticsearch-test -p 9200:9200 plugin-elasticsearch-test

until curl -sf -u elastic:resonate "http://localhost:9200/_cluster/health?wait_for_status=yellow&timeout=10s" >/dev/null; do sleep 5; done
until ELASTICSEARCH_API_KEY=$(curl -sf -u elastic:resonate -X POST http://localhost:9200/_security/api_key -H "Content-Type: application/json" -d '{"name": "resonate"}' | python3 -c "import json,sys; print(json.load(sys.stdin)['encoded'])") && [ -n "$ELASTICSEARCH_API_KEY" ]; do sleep 5; done
until curl -sf -H "Authorization: ApiKey $ELASTICSEARCH_API_KEY" http://localhost:9200/_security/_authenticate >/dev/null; do sleep 5; done

curl -sf -X PUT http://localhost:9200/resonate_fixture_ok -H "Authorization: ApiKey $ELASTICSEARCH_API_KEY" -H "Content-Type: application/json" -d '{"mappings": {"properties": {"n": {"type": "long"}}}}' >/dev/null
python3 -c "
for i in range(200):
    print('{\"index\": {\"_id\": \"%d\"}}' % i)
    print('{\"n\": %d}' % i)
" | curl -sf -X POST "http://localhost:9200/resonate_fixture_ok/_bulk?refresh=true" -H "Authorization: ApiKey $ELASTICSEARCH_API_KEY" -H "Content-Type: application/x-ndjson" --data-binary @- >/dev/null

curl -sf -X PUT http://localhost:9200/resonate_fixture_fail -H "Authorization: ApiKey $ELASTICSEARCH_API_KEY" -H "Content-Type: application/json" -d '{"mappings": {"_source": {"enabled": false}}}' >/dev/null
curl -sf -X POST "http://localhost:9200/resonate_fixture_fail/_doc?refresh=true" -H "Authorization: ApiKey $ELASTICSEARCH_API_KEY" -H "Content-Type: application/json" -d '{"n": 1}' >/dev/null

export ELASTICSEARCH_BASE_URL=http://localhost:9200
export ELASTICSEARCH_API_KEY
export ELASTICSEARCH_FIXTURE_OK=resonate_fixture_ok
export ELASTICSEARCH_FIXTURE_FAIL=resonate_fixture_fail
```
