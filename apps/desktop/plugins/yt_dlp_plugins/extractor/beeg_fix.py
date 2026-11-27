from yt_dlp.extractor.beeg import BeegIE
from yt_dlp.utils import int_or_none, str_or_none, traverse_obj, unified_timestamp


class BeegFixIE(BeegIE, plugin_name='ytdlpd'):
    """Beeg moved HLS urls out of facts/file (hls_resources) into /video/play_url/<id>."""

    def _real_extract(self, url):
        video_id = self._match_id(url)
        # thumbnails link /-0<id>; the leading 0 is a url marker, the API wants the bare id
        video_id = video_id.lstrip('0') or video_id
        video = self._download_json(
            f'https://store.externulls.com/facts/file/{video_id}', video_id,
            f'Downloading JSON for {video_id}')
        fact = min((f for f in video.get('fc_facts') or [] if f.get('id') is not None),
                   key=lambda f: f['id'], default={})
        # legacy layout still works when the API serves it again
        if traverse_obj(video, ('file', 'hls_resources')) or fact.get('hls_resources'):
            return super()._real_extract(url)

        play_path = self._download_webpage(
            f'https://store.externulls.com/video/play_url/{video_id}', video_id,
            'Downloading play url', headers={'Origin': 'https://beeg.com', 'Referer': 'https://beeg.com/'})
        formats = self._extract_m3u8_formats(
            f'https://video.beeg.com/{play_path.strip().lstrip("/")}', video_id, 'mp4', m3u8_id='hls',
            headers={'Origin': 'https://beeg.com', 'Referer': 'https://beeg.com/'})
        title = traverse_obj(video, ('file', 'data', lambda _, v: v.get('cd_column') == 'sf_name', 'cd_value', any))
        return {
            'id': video_id,
            'display_id': str_or_none(fact.get('id')),
            'title': title or f'beeg {video_id}',
            'timestamp': unified_timestamp(fact.get('fc_created')),
            'duration': int_or_none(traverse_obj(video, ('file', 'fl_duration'))),
            'tags': traverse_obj(video, ('tags', ..., 'tg_name')),
            'formats': formats,
            'age_limit': 18,
        }
