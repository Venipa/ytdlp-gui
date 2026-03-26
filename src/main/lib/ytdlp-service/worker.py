#!/usr/bin/env python3
# encoding: utf-8
# -*- coding: utf-8 -*-
# SPDX-License-Identifier: LicenseRef-Proprietary
# Copyright (c) 2026 Venipa. All rights reserved.
#
# This file is proprietary. It may not be copied, modified, distributed, or used
# except by the copyright holder (Venipa) without explicit written permission.
# Repository licensing: see LICENSING.md at the project root.

"""
JSON-RPC worker for yt-dlp operations.
Communicates via JSON typed RPC protocol over stdin/stdout.
"""

import json
import os
import sys
import traceback
from datetime import datetime
from io import FileIO
from typing import Any, Callable, Dict, List, Optional, Tuple

# Assume we have yt-dlp installed
try:
    import yt_dlp
    from yt_dlp import YoutubeDL
    from yt_dlp import parse_options as ytdlp_parse_options
    from yt_dlp.options import create_parser as ytdlp_create_parser
    from yt_dlp.version import __version__ as YTDLP_VERSION
except ImportError:
    YoutubeDL = None
    YTDLP_VERSION = None


class Logger:
    def __logger_call(self, level: str, message: str):
        print(f"[{level}][{self.name}] {message}")
        self.file.write(f"[{level}][{self.name}] {message}\n")
        if not self.file.closed:
            self.file.flush()

    def __open_file(self):
        date_day = datetime.now().strftime("%Y-%m-%d")
        logger_name = f"{self.name.lower().replace(' ', '_')}_{date_day}.log"
        self.file = open(
            os.path.join(os.path.dirname(__file__), logger_name), "a", encoding="utf-8"
        )

    def __init__(self, name: str, file: FileIO | None = None):
        self.name = name
        self.file = None
        self.__open_file()
        self.catch_all_unhandled_exceptions()

    def catch_all_unhandled_exceptions(self):
        """
        Install hooks to catch and log all unhandled exceptions and uncaught stderr output.
        """
        import sys

        def excepthook(exc_type, exc_value, exc_tb):
            self.error("Unhandled exception:")
            self.error("".join(traceback.format_exception(exc_type, exc_value, exc_tb)))

        sys.excepthook = excepthook

        class StderrLogger:
            def __init__(self, logger_instance):
                self.logger = logger_instance

            def write(self, message):
                message = str(message)
                if message.strip():
                    self.logger.error(message.rstrip())

            def flush(self):
                pass

        sys.stderr = StderrLogger(self)

    def __del__(self):
        self.file.close()

    def info(self, message: str):
        self.__logger_call("INFO", message)

    def error(self, message: str):
        self.__logger_call("ERROR", message)

    def warn(self, message: str):
        self.__logger_call("WARN", message)

    def debug(self, message: str):
        self.__logger_call("DEBUG", message)


WORKER_ID = os.environ.get("YTDLP_WORKER_ID")


def write_json(payload: Dict[str, Any]) -> None:
    """Write a JSON object as one line and flush."""
    sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def create_progress_hook(request_id: str):
    def _progress_hook(progress: Dict[str, Any]) -> None:
        status = progress.get("status")
        info = progress.get("info_dict") or {}
        video_id = info.get("id") or "unknown"
        total_bytes = progress.get("total_bytes") or progress.get(
            "total_bytes_estimate"
        )
        downloaded_bytes = progress.get("downloaded_bytes")
        percent_value = None
        speed = None
        eta = None
        if (
            isinstance(total_bytes, (int, float))
            and total_bytes > 0
            and isinstance(downloaded_bytes, (int, float))
        ):
            percent_value = max(
                0.0, min(100.0, (float(downloaded_bytes) / float(total_bytes)) * 100.0)
            )

        if status == "finished":
            # Do not emit completion-like progress here:
            # postprocessors (e.g. merger) may still be running.
            return
        elif status == "downloading":
            percent = (
                progress.get("_percent_str")
                or (f"{percent_value:.1f}%" if percent_value is not None else "0.0%")
            ).strip()
            total = (
                progress.get("_total_bytes_str")
                or progress.get("_total_bytes_estimate_str")
                or ""
            ).strip()
            speed = (progress.get("_speed_str") or "").strip()
            eta = (progress.get("_eta_str") or "").strip()
            if eta.upper().startswith("ETA"):
                eta = eta[3:].lstrip(": ").strip()
            if eta.upper().endswith("ETA"):
                eta = eta[:-3].rstrip()

            message_parts = [f"{percent} of {total}".strip()]
            if speed:
                message_parts.append(f"at {speed}")
            if eta:
                message_parts.append(f"ETA {eta}")
            fragment_index = progress.get("_fragment_index")
            fragment_count = progress.get("_fragment_count")
            if fragment_index is not None and fragment_count is not None:
                message_parts.append(f"(frag {fragment_index}/{fragment_count})")
            line = f"[download] {video_id}: {' '.join(message_parts)}"
        else:
            return

        write_json(
            {
                "__ytdlp_progress__": True,
                "id": request_id,
                "line": line,
                "workerId": WORKER_ID,
                "status": status,
                "videoId": video_id,
                "percent": percent_value,
                "speed": speed,
                "eta": eta,
                "fragmentIndex": progress.get("_fragment_index"),
                "fragmentCount": progress.get("_fragment_count"),
            }
        )

    return _progress_hook


def merge_options(options: Any) -> Dict[str, Any]:
    """Normalize options and keep yt-dlp stderr/stdout clean for RPC parsing."""
    opts = dict(options) if isinstance(options, dict) else {}
    opts["quiet"] = True
    opts["no_warnings"] = True
    cliargs = opts.get("cliargs", None)
    ytdlOptions: Dict[str, Any] = {}
    if cliargs and isinstance(cliargs, list) and len(cliargs) > 0:
      ytdlOptions = dict(transform_cliargs(cliargs))
    else:
      ytdlOptions = dict(ytdlp_parse_options([]).ydl_opts)
    ytdlOptions.update(opts)

    return ytdlOptions


def rpc_response(
    id: str, result: Optional[Any] = None, error: Optional[str] = None
) -> Dict[str, Any]:
    """Create a JSON-RPC compliant response."""
    resp: Dict[str, Any] = {"id": id}
    if error is not None:
        resp["error"] = error
    else:
        resp["result"] = result
    return resp


def get_yt_dlp_version() -> str:
    """Get the version of yt-dlp."""
    if not YoutubeDL:
        return "yt-dlp not installed."
    # Mirrors yt_dlp/YoutubeDL.py, which imports __version__ from yt_dlp.version
    if YTDLP_VERSION:
        return str(YTDLP_VERSION)
    instance_version = getattr(YoutubeDL(), "_version", None)
    if instance_version:
        return str(instance_version)
    return "unknown"


def handle_ready(id: str, params: Dict[str, Any]) -> Dict[str, Any]:
    """Handle ready RPC call."""
    if not YoutubeDL:
        return rpc_response(id, error="yt-dlp not installed.")
    try:
        return rpc_response(
            id,
            result={
                "version": get_yt_dlp_version(),
            },
        )
    except Exception as e:
        tb = traceback.format_exc()
        return rpc_response(id, error=f"{type(e).__name__}: {str(e)}\n{tb}")


def handle_extract_info(id: str, params: Dict[str, Any]) -> Dict[str, Any]:
    """Handle extract_info RPC call."""
    url = params.get("url")
    options = merge_options(params.get("options", {}))
    outtmpl = options.get("outtmpl", None)

    if not url:
        return rpc_response(id, error="Missing required parameter: url")

    if not YoutubeDL:
        return rpc_response(id, error="yt-dlp not installed.")

    try:
        with YoutubeDL(options) as ydl:
            info = ydl.extract_info(url, download=False)
            if outtmpl:
                info.update(
                    {
                        "filename": (
                            ydl.prepare_filename(
                                info_dict=info,
                                outtmpl=outtmpl,
                            )
                            or None
                        )
                    }
                )
        return rpc_response(id, result=info)
    except Exception as e:
        tb = traceback.format_exc()
        return rpc_response(id, error=f"{type(e).__name__}: {str(e)}\n{tb}")


def handle_get_version(id: str, params: Dict[str, Any]) -> Dict[str, Any]:
    """Handle get_version RPC call."""
    if not YoutubeDL:
        return rpc_response(id, error="yt-dlp not installed.")
    try:
        version = get_yt_dlp_version()
        return rpc_response(id, result=version)
    except Exception as e:
        tb = traceback.format_exc()
        return rpc_response(id, error=f"{type(e).__name__}: {str(e)}\n{tb}")


def handle_quit(id: str, params: Dict[str, Any]) -> Dict[str, Any]:
    """Handle quit RPC call."""
    sys.exit(0)
    return rpc_response(id, result="Quitting...")


def ensure_download_success(exit_code: Any) -> None:
    """Raise if yt-dlp reports a non-zero download result code."""
    if isinstance(exit_code, int) and exit_code != 0:
        raise RuntimeError(f"yt-dlp download failed with exit code {exit_code}")

def parse_patched_options(opts, defaults: Dict[str, Any] | None = None):
    patched_parser = ytdlp_create_parser()
    patched_parser.defaults.update(defaults or {})
    yt_dlp.options.create_parser = lambda: patched_parser
    try:
        return yt_dlp.parse_options(opts)
    finally:
        yt_dlp.options.create_parser = ytdlp_create_parser


default_opts = parse_patched_options([]).ydl_opts


def transform_cliargs(opts, defaults: Dict[str, Any] | None = None):
    opts = (yt_dlp.parse_options if defaults else parse_patched_options)(opts).ydl_opts

    diff = {k: v for k, v in opts.items() if default_opts[k] != v}
    if 'postprocessors' in diff:
        diff['postprocessors'] = [pp for pp in diff['postprocessors']
                                  if pp not in default_opts['postprocessors']]
    return diff


def handle_download(id: str, params: Dict[str, Any]) -> Dict[str, Any]:
    """Handle download RPC call."""
    url = params.get("url")
    options = merge_options(params.get("options", {}))
    info_ref = options.get("info", None)

    if not url:
        return rpc_response(id, error="Missing required parameter: url")

    if not YoutubeDL:
        return rpc_response(id, error="yt-dlp not installed.")
    try:
        existing_hooks = (
            options.get("progress_hooks")
            if isinstance(options.get("progress_hooks"), list)
            else []
        )
        options["progress_hooks"] = [*existing_hooks, create_progress_hook(id)]

        with YoutubeDL(options) as ydl:
            ydl.download([url])

        write_json(
            {
                "__ytdlp_progress__": True,
                "id": id,
                "line": ("[download] postprocess: completed"),
                "workerId": WORKER_ID,
                "status": "completed",
                "videoId": None,
                "percent": 100.0,
                "speed": None,
                "eta": None,
                "fragmentIndex": None,
                "fragmentCount": None,
            }
        )
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
_hostname_postprocessors: Dict[str, List[Dict[str, Any]]] = {
    "coub.com": [
        {
            "key": "Merger+ffmpeg_i1",
            "value": "-stream_loop -1",
        },
        {
            "key": "Merger+ffmpeg_o1",
            "value": "-shortest",
        },
    ]
}


def get_hostname(urlString: str) -> str:
    """
    Extract and return the hostname (domain) from a URL string, without using urllib or other libraries.
    Returns empty string if extraction fails.
    """
    # Remove protocol
    if "://" in urlString:
        url_no_proto = urlString.split("://", 1)[1]
    else:
        url_no_proto = urlString
    # Hostname is up to first '/' or end of string
    slash_index = url_no_proto.find("/")
    if slash_index != -1:
        host_port = url_no_proto[:slash_index]
    else:
        host_port = url_no_proto
    # Remove port if any
    host = host_port.split(":")[0]
    # Return host, or empty string if not found
    return host if host else ""


def get_hostname_postprocessors(urlString: str) -> List[Dict[str, Any]]:
    hostname = get_hostname(urlString)
    if hostname in _hostname_postprocessors:
        return _hostname_postprocessors[hostname]
    return []


def normalize_rpc_params(
    raw_params: Any,
) -> Tuple[Optional[Dict[str, Any]], Optional[str]]:
    """Accept params as JSON string or object and normalize to object."""
    if isinstance(raw_params, dict):
        return raw_params, None
    if isinstance(raw_params, str):
        try:
            parsed = json.loads(raw_params)
        except json.JSONDecodeError as e:
            return None, f"Invalid params JSON string: {str(e)}"
        if not isinstance(parsed, dict):
            return None, "Invalid params JSON string: must decode to an object"
        return parsed, None
    return None, "Invalid params: must be an object or JSON object string"


logger = Logger("worker_" + WORKER_ID)


def main() -> None:
    logger.info("Worker started")
    initialized = False
    version = None
    if YoutubeDL:
        try:
            version = get_yt_dlp_version()
        except Exception:
            version = None

    while True:
        try:
            if not initialized:
                # Startup handshake for the TS wrapper.
                # The wrapper waits for id="ready" before sending calls.
                resp = rpc_response(id="ready", result={"version": version})
                write_json(resp)
                initialized = True

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
                resp = rpc_response(id="", error=f"Invalid JSON: {str(e)}")
                write_json(resp)
                continue

            # Extract RPC request fields
            req_id = req.get("id")
            method = req.get("method")
            raw_params = req.get("params", {})
            req_id_value = req_id if isinstance(req_id, str) else ""
            params, params_error = normalize_rpc_params(raw_params)

            # Validate request structure
            if not req_id_value:
                resp = rpc_response(id="", error="Missing required field: id (string)")
            elif not method:
                resp = rpc_response(
                    id=req_id_value, error="Missing required field: method"
                )
            elif params_error is not None:
                resp = rpc_response(id=req_id_value, error=params_error)
            else:
                # Route to handler
                handler = _rpc_methods.get(method)
                if not handler:
                    resp = rpc_response(
                        id=req_id_value, error=f"Unknown method: {method}"
                    )
                else:
                    try:
                        resp = handler(req_id_value, params)
                    except Exception as e:
                        tb = traceback.format_exc()
                        resp = rpc_response(
                            id=req_id_value,
                            error=f"Handler exception: {type(e).__name__}: {str(e)}\n{tb}",
                        )

            # Send JSON response
            write_json(resp)

        except KeyboardInterrupt:
            break
        except Exception as e:
            tb = traceback.format_exc()
            resp = rpc_response(
                id="", error=f"Fatal error: {type(e).__name__}: {str(e)}\n{tb}"
            )
            logger.error(
                f"Fatal error: {type(e).__name__}: {str(e)}\n{tb or 'No traceback available'}"
            )
            try:
                write_json(resp)
            except Exception:
                pass
            break


if __name__ == "__main__":
    main()
