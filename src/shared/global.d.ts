declare module 'ytdlp-desktop' {
  export { };
}
declare module 'ytdlp-desktop/types' {
  export type YTDLState = 'progressing' | 'done'
  export type YTDLStatus =
    | { action: string; data?: any; state: YTDLState }
    | { action: string; error: any; state: YTDLState }
  export type YTDLDownloadStatus =
    | {
        percent?: number
        totalSize?: string
        currentSpeed?: string
        eta?: string
      }
    | { percent: number; error: any }

  export interface YTDLItem {
    id: number
    state: string
    title: string
    filesize: number
    type: string
    source: string
    url: string
    filepath: string
    retryCount: number
    error?: any
  }
}

declare module 'yt-dlp-wrap/types' {
  type VideoQuality =
    | '2160p'
    | '1440p'
    | '1080p'
    | '720p'
    | '480p'
    | '360p'
    | '240p'
    | '144p'
    | 'highest'
    | 'lowest'

  type StreamQualityOptions = {
    videoonly: VideoQuality
    audioonly: 'highest' | 'lowest'
    audioandvideo: 'highest' | 'lowest'
  }
  type DownloadQualityOptions = {
    videoonly: VideoQuality
    audioonly: 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10
    audioandvideo: 'highest' | 'lowest'
    mergevideo: VideoQuality
  }

  type DownloadFormatOptions = {
    videoonly: 'mp4' | 'webm'
    audioandvideo: 'mp4' | 'webm'
    mergevideo: 'mkv' | 'mp4' | 'ogg' | 'webm' | 'flv'
    audioonly: 'aac' | 'flac' | 'mp3' | 'm4a' | 'opus' | 'vorbis' | 'wav' | 'alac'
  }

  export type OutputType = {
    output?:
      | {
          outDir: string
          fileName?: string | 'default'
        }
      | string
  }

  //! export type from here
  export type StreamKeyWord = keyof StreamQualityOptions

  export type StreamOptions<F extends StreamKeyWord> = {
    filter: F
    quality?: StreamQualityOptions[F]
    command?: string[]
  }

  export type DownloadKeyWord = keyof DownloadQualityOptions

  export type DownloadOptions<F extends DownloadKeyWord> = (F extends 'mergevideo'
    ? {
        filter: F
        quality?: DownloadQualityOptions[F]
        format?: DownloadFormatOptions[F]
        embedSubs?: boolean
        embedThumbnail?: boolean
        command?: string[]
      }
    : {
        filter: F
        quality?: DownloadQualityOptions[F]
        format?: DownloadFormatOptions[F]
        embedSubs?: boolean
        embedThumbnail?: boolean
        command?: string[]
      }) &
    OutputType

  export type AsyncOptions<F extends DownloadKeyWord> = {
    onProgress: () => void
  } & DownloadOptions<F>

  export type PipeType<T> = (
    destination: NodeJS.WritableStream,
    options?: {
      end?: boolean
    }
  ) => T

  export interface VideoInfo {
    id: string
    title: string
    formats: Format[]
    thumbnails: VideoThumbnail[]
    thumbnail: string
    description: string
    channel_id: string
    channel_url: string
    duration: number
    view_count: number
    average_rating: null
    age_limit: number
    webpage_url: string
    categories: string[]
    tags: any[]
    playable_in_embed: boolean
    live_status: string
    media_type: null
    release_timestamp: null
    _format_sort_fields: string[]
    automatic_captions: { [key: string]: AutomaticCaption[] }
    subtitles: VideoSubtitles
    comment_count: number
    chapters: null
    heatmap: VideoHeatmap[]
    like_count: number
    channel: string
    channel_follower_count: number
    uploader: string
    uploader_id: string
    uploader_url: string
    upload_date: string
    timestamp: number
    availability: string
    original_url: string
    webpage_url_basename: string
    webpage_url_domain: string
    extractor: string
    extractor_key: string
    playlist: null
    playlist_index: null
    display_id: string
    fulltitle: string
    duration_string: string
    release_year: null
    is_live: boolean
    was_live: boolean
    requested_subtitles: null
    _has_drm: null
    epoch: number
    asr: number
    filesize: number
    format_id: string
    format_note: string
    source_preference: number
    fps: number
    audio_channels: number
    height: number
    quality: number
    has_drm: boolean
    tbr: number
    filesize_approx: number
    url: string
    width: number
    language: VideoLanguage
    language_preference: number
    preference: null
    ext: VideoEXTEnum
    vcodec: string
    acodec: VideoAcodec
    dynamic_range: VideoDynamicRange
    downloader_options: VideoDownloaderOptions
    protocol: VideoProtocol
    video_ext: VideoEXTEnum
    audio_ext: VideoAcodec
    vbr: null
    abr: null
    resolution: string
    aspect_ratio: number
    format: string
    _filename: string
    filename: string
    _type: string
    _version: Version
  }


  export enum VideoAcodec {
    Mp4A402 = 'mp4a.40.2',
    None = 'none',
    Opus = 'opus'
  }

  export interface AutomaticCaption {
    ext: VideoAutomaticCaptionEXT
    url: string
    name: string
  }

  export enum VideoAutomaticCaptionEXT {
    Json3 = 'json3',
    Srv1 = 'srv1',
    Srv2 = 'srv2',
    Srv3 = 'srv3',
    Ttml = 'ttml',
    Vtt = 'vtt'
  }

  export interface VideoDownloaderOptions {
    http_chunk_size: number
  }

  export enum VideoDynamicRange {
    SDR = 'SDR'
  }

  export enum VideoEXTEnum {
    M4A = 'm4a',
    Mhtml = 'mhtml',
    Mp4 = 'mp4',
    None = 'none',
    Webm = 'webm'
  }

  export interface Format {
    format_id: string
    format_note?: string
    ext: VideoEXTEnum
    protocol: VideoProtocol
    acodec?: VideoAcodec
    vcodec: string
    url: string
    width?: number | null
    height?: number | null
    fps?: number | null
    rows?: number
    columns?: number
    fragments?: Fragment[]
    audio_ext: VideoEXTEnum
    video_ext: VideoEXTEnum
    vbr: number | null
    abr: number | null
    tbr: number | null
    resolution: string
    aspect_ratio: number | null
    filesize_approx?: number | null
    http_headers: HTTPHeaders
    format: string
    format_index?: null
    manifest_url?: string
    language?: VideoLanguage | null
    preference?: null
    quality?: number
    has_drm?: boolean
    source_preference?: number
    asr?: number | null
    filesize?: number
    audio_channels?: number | null
    language_preference?: number
    dynamic_range?: VideoDynamicRange | null
    container?: VideoContainer
    downloader_options?: VideoDownloaderOptions
  }

  export enum VideoContainer {
    M4ADash = 'm4a_dash',
    Mp4Dash = 'mp4_dash',
    WebmDash = 'webm_dash'
  }

  export interface Fragment {
    url: string
    duration: number
  }



  export enum VideoLanguage {
    En = 'en'
  }

  export enum VideoProtocol {
    HTTPS = 'https',
    M3U8Native = 'm3u8_native',
    Mhtml = 'mhtml'
  }

  export interface VideoHeatmap {
    start_time: number
    end_time: number
    value: number
  }

  export interface VideoSubtitles {}

  export interface VideoThumbnail {
    url: string
    preference: number
    id: string
    height?: number
    width?: number
    resolution?: string
  }

  export type ThumbnailsOptions = {
    quality?: 'max' | 'hq' | 'mq' | 'sd' | 'default'
    type?: 'jpg' | 'webp'
  }

  export type ProgressType = {
    status: 'downloading' | 'finished'
    downloaded: number
    downloaded_str: string
    total: number
    total_str: string
    speed: number
    speed_str: string
    eta: number
    eta_str: string
    percentage: number
    percentage_str: string
  }
}
