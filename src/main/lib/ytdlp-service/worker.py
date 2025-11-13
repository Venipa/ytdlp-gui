#!/usr/bin/env python3
"""
JSON-RPC worker for yt-dlp operations.
Communicates via JSON typed RPC protocol over stdin/stdout.
"""
import sys
import json
import traceback
from typing import Dict, Any, Optional, Callable

# Assume we have yt-dlp installed
try:
    from yt_dlp import YoutubeDL
except ImportError:
    YoutubeDL = None


def rpc_response(id: str, result: Optional[Any] = None, error: Optional[str] = None) -> Dict[str, Any]:
    """Create a JSON-RPC compliant response."""
    resp: Dict[str, Any] = {"id": id}
    if error is not None:
        resp["error"] = error
    else:
        resp["result"] = result
    return resp


def handle_ready(id: str, params: Dict[str, Any]) -> Dict[str, Any]:
    """Handle ready RPC call."""
    try:
      return rpc_response(id, result={
        "version": YoutubeDL()._version,
      })
    except Exception as e:
        tb = traceback.format_exc()
        return rpc_response(id, error=f"{type(e).__name__}: {str(e)}\n{tb}")


def handle_extract_info(id: str, params: Dict[str, Any]) -> Dict[str, Any]:
    """Handle extract_info RPC call."""
    url = params.get("url")
    options = params.get("options", {})

    if not url:
        return rpc_response(id, error="Missing required parameter: url")

    if not YoutubeDL:
        return rpc_response(id, error="yt-dlp not installed.")

    try:
        with YoutubeDL(options) as ydl:
            info = ydl.extract_info(url, download=False)
        return rpc_response(id, result=info)
    except Exception as e:
        tb = traceback.format_exc()
        return rpc_response(id, error=f"{type(e).__name__}: {str(e)}\n{tb}")


def handle_get_version(id: str, params: Dict[str, Any]) -> Dict[str, Any]:
    """Handle get_version RPC call."""
    if not YoutubeDL:
        return rpc_response(id, error="yt-dlp not installed.")
    try:
        version = YoutubeDL()._version
        return rpc_response(id, result=version)
    except Exception as e:
        tb = traceback.format_exc()
        return rpc_response(id, error=f"{type(e).__name__}: {str(e)}\n{tb}")


def handle_quit(id: str, params: Dict[str, Any]) -> Dict[str, Any]:
    """Handle quit RPC call."""
    sys.exit(0)
    return rpc_response(id, result="Quitting...")


def handle_download(id: str, params: Dict[str, Any]) -> Dict[str, Any]:
    """Handle download RPC call."""
    url = params.get("url")
    options = params.get("options", {})

    if not url:
        return rpc_response(id, error="Missing required parameter: url")

    if not YoutubeDL:
        return rpc_response(id, error="yt-dlp not installed.")

    try:
        with YoutubeDL(options) as ydl:
            ydl.download([url])
        return rpc_response(id, result="Download completed")
    except Exception as e:
        tb = traceback.format_exc()
        return rpc_response(id, error=f"{type(e).__name__}: {str(e)}\n{tb}")


# RPC method registry
_rpc_methods: Dict[str, Callable[[str, Dict[str, Any]], Dict[str, Any]]] = {
    "ready": handle_ready,
    "download": handle_download,
    "extract_info": handle_extract_info,
    "get_version": handle_get_version,
    "quit": handle_quit,
}


def main() -> None:
  initialized = False
  version = YoutubeDL()._version
    """Main JSON-RPC loop: read JSON requests from stdin, write JSON responses to stdout."""
    while True:
        try:

          if not initialized:
            sys.stdout.write(json.dumps(rpc_response(id="", result={"version": version})) + "\n")
            sys.stdout.flush()
            initialized = True
            continue

            line = sys.stdin.readline()
            if not line:
                break  # EOF

            line = line.strip()
            if not line:
                continue  # Skip empty lines

            # Parse JSON RPC request
            try:
                req: Dict[str, Any] = json.loads(line)
            except json.JSONDecodeError as e:
                resp = rpc_response(
                    id="",
                    error=f"Invalid JSON: {str(e)}"
                )
                sys.stdout.write(json.dumps(resp) + "\n")
                sys.stdout.flush()
                continue

            # Extract RPC request fields
            req_id = req.get("id")
            method = req.get("method")
            params = req.get("params", {})

            # Validate request structure
            if not req_id:
                resp = rpc_response(
                    id="",
                    error="Missing required field: id"
                )
            elif not method:
                resp = rpc_response(
                    id=req_id if isinstance(req_id, str) else "",
                    error="Missing required field: method"
                )
            elif not isinstance(params, dict):
                resp = rpc_response(
                    id=req_id if isinstance(req_id, str) else "",
                    error="Invalid params: must be an object"
                )
            else:
                # Route to handler
                handler = _rpc_methods.get(method)
                if not handler:
                    resp = rpc_response(
                        id=req_id if isinstance(req_id, str) else "",
                        error=f"Unknown method: {method}"
                    )
                else:
                    try:
                        resp = handler(req_id if isinstance(req_id, str) else "", params)
                    except Exception as e:
                        tb = traceback.format_exc()
                        resp = rpc_response(
                            id=req_id if isinstance(req_id, str) else "",
                            error=f"Handler exception: {type(e).__name__}: {str(e)}\n{tb}"
                        )

            # Send JSON response
            sys.stdout.write(json.dumps(resp, ensure_ascii=False) + "\n")
            sys.stdout.flush()

        except KeyboardInterrupt:
            break
        except Exception as e:
            tb = traceback.format_exc()
            resp = rpc_response(
                id="",
                error=f"Fatal error: {type(e).__name__}: {str(e)}\n{tb}"
            )
            try:
                sys.stdout.write(json.dumps(resp) + "\n")
                sys.stdout.flush()
            except Exception:
                pass
            break


if __name__ == "__main__":
    main()
