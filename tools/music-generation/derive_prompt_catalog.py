"""Derive a provider-neutral music taxonomy and deterministic prompt example.

The input is a browser HAR containing a JSON session response.  Only musical
metadata is retained: authentication headers, cookies, URLs, opaque IDs, and
track names are deliberately ignored.  The resulting catalog keeps both the
distinct vocabulary and the co-occurrence profile needed to build prompts
without turning the taxonomy into an unstructured adjective list.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, Iterable


TAG_TYPES = ("activity", "genre", "subgenre", "mood", "instrument", "release")

ROLE_GUIDANCE = {
    "acoustic bass": "a rounded acoustic bass foundation, controlled and supportive",
    "acoustic drum set": "a natural acoustic drum-set pulse with restrained fills",
    "acoustic drumset": "a natural acoustic drum-set pulse with restrained fills",
    "acoustic piano": "a warm acoustic piano as the harmonic anchor and motif carrier",
    "arp synth": "a restrained arpeggiated pulse in the background, never a piercing lead",
    "arp synth bass": "a soft arpeggiated low-mid support layer, never sub-bass heavy",
    "chimes/bells": "very sparse soft metallic accents only when they do not distract",
    "choral voices": "instrumental string and brass layers in place of any choir or voice",
    "electric bass": "a rounded electric bass groove with clean midrange definition",
    "electric guitar": "a warm clean electric-guitar texture, no rock lead or solo",
    "electric keys": "rounded electric-key chords with natural attack and decay",
    "electronic percussion": "controlled electronic percussion with a steady even pulse",
    "ethnic percussion": "organic hand percussion used sparingly for human movement",
    "ethnic strings": "a warm plucked or bowed string color used as a supporting texture",
    "horns": "rounded middle-register horn support with natural phrasing",
    "mallets": "soft, rounded mallet punctuation without bright repetitive hooks",
    "orchestral brass": "rounded orchestral brass in comfortable middle registers",
    "orchestral percussion": "restrained orchestral percussion for gradual forward motion",
    "orchestral strings": "an expressive string ensemble carrying evolving harmony",
    "orchestral winds": "warm orchestral winds used for quiet counter-melody and color",
    "organic percussion": "sparse organic percussion accents with humanized dynamics",
    "pedal steel": "a restrained warm pedal-steel color, never a foreground solo",
    "processed strings": "wide processed string texture with slow evolving movement",
    "synth bass": "a warm controlled synth-bass foundation focused in the low-mid range",
    "synth pad": "a soft atmospheric synth pad that supports the harmony without masking it",
    "textural soundscape": "a low-contrast textural soundscape that leaves space for the motif",
}

VOCAL_LIKE = {"processed vocals", "choral voices"}


def unique_sorted(values: Iterable[Any]) -> list[str]:
    return sorted({str(value).strip() for value in values if str(value).strip()}, key=str.casefold)


def tag_values(tags: Iterable[dict[str, Any]], tag_type: str) -> list[str]:
    return unique_sorted(
        tag.get("value")
        for tag in tags
        if isinstance(tag, dict) and tag.get("type") == tag_type
    )


def load_response(har_path: Path) -> tuple[dict[str, Any], dict[str, Any]]:
    try:
        har = json.loads(har_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"cannot read HAR: {error}") from error

    entries = har.get("log", {}).get("entries", [])
    if not isinstance(entries, list) or not entries:
        raise ValueError("HAR has no request entries")

    for entry in entries:
        content = entry.get("response", {}).get("content", {})
        raw_text = content.get("text")
        if not isinstance(raw_text, str) or not raw_text.strip():
            continue
        try:
            response = json.loads(raw_text)
        except json.JSONDecodeError:
            continue
        if isinstance(response, dict) and isinstance(response.get("result", {}).get("servings"), list):
            return entry, response

    raise ValueError("HAR does not contain a JSON session response with track servings")


def profile_from_serving(index: int, serving: dict[str, Any]) -> dict[str, Any]:
    track = serving.get("track", {})
    tags = track.get("tags", [])
    related_activities = [
        activity.get("displayValue")
        for activity in track.get("relatedActivities", [])
        if isinstance(activity, dict)
    ]
    variations = [
        variation
        for variation in track.get("variations", [])
        if isinstance(variation, dict)
    ]
    current_variation = serving.get("trackVariation", {})

    return {
        "profile_id": f"profile-{index:03d}",
        "activities": unique_sorted(
            [*tag_values(tags, "activity"), *related_activities]
        ),
        "genres": tag_values(tags, "genre"),
        "subgenres": tag_values(tags, "subgenre"),
        "moods": tag_values(tags, "mood"),
        "instruments": tag_values(tags, "instrument"),
        "bpm": track.get("beatsPerMinute"),
        "brightness": track.get("brightnessLevel"),
        "complexity": track.get("complexityLevel"),
        "mental_state": (track.get("mentalState") or {}).get("displayValue"),
        "variation_styles": unique_sorted(
            variation.get("style") for variation in variations
        ),
        "variation_lengths_seconds": sorted(
            {
                int(variation["lengthInSeconds"])
                for variation in variations
                if isinstance(variation.get("lengthInSeconds"), (int, float))
            }
        ),
        "neural_effect_level": current_variation.get("neuralEffectLevel"),
    }


def numeric_dimension(profiles: list[dict[str, Any]], key: str) -> dict[str, Any]:
    values = sorted(
        {
            value
            for profile in profiles
            if isinstance((value := profile.get(key)), (int, float))
        }
    )
    if not values:
        return {"min": None, "max": None, "observed": []}
    return {"min": values[0], "max": values[-1], "observed": values}


def derive_catalog(har_path: Path, generated_on: str) -> dict[str, Any]:
    entry, response = load_response(har_path)
    servings = response["result"]["servings"]
    profiles = [
        profile_from_serving(index, serving)
        for index, serving in enumerate(servings, start=1)
        if isinstance(serving, dict)
    ]

    request_genres: list[str] = []
    request_text = entry.get("request", {}).get("postData", {}).get("text")
    if isinstance(request_text, str) and request_text.strip():
        try:
            request = json.loads(request_text)
        except json.JSONDecodeError:
            request = {}
        request_genres = unique_sorted(request.get("genreNames", []))

    observed = {
        dimension: unique_sorted(
            value
            for profile in profiles
            for value in profile.get(dimension, [])
        )
        for dimension in ("activities", "genres", "subgenres", "moods", "instruments")
    }
    all_genres = unique_sorted([*observed["genres"], *request_genres])

    return {
        "schema": "aria-focus.music-prompt-catalog",
        "schema_version": 1,
        "generated_on": generated_on,
        "profile_count": len(profiles),
        "dimensions": {
            "activities": observed["activities"],
            "genres": all_genres,
            "observed_track_genres": observed["genres"],
            "selector_genres": request_genres,
            "subgenres": observed["subgenres"],
            "moods": observed["moods"],
            "instruments": observed["instruments"],
            "mental_states": unique_sorted(profile.get("mental_state") for profile in profiles),
            "variation_styles": unique_sorted(
                style
                for profile in profiles
                for style in profile.get("variation_styles", [])
            ),
            "neural_effect_levels": unique_sorted(
                ["high"]
                if request_text and "neuralEffectLevels" in request_text
                else []
            ),
        },
        "numeric_dimensions": {
            "bpm": numeric_dimension(profiles, "bpm"),
            "brightness": {
                **numeric_dimension(profiles, "brightness"),
                "scale": "0..1; lower is darker/warmer, higher is brighter",
            },
            "complexity": {
                **numeric_dimension(profiles, "complexity"),
                "scale": "0..1; lower is sparser, higher is denser/more active",
            },
            "neural_effect_level": numeric_dimension(profiles, "neural_effect_level"),
        },
        "profiles": profiles,
        "privacy": {
            "retained": "musical vocabulary, co-occurrence, and numeric descriptors only",
            "removed": "track names, opaque identifiers, audio URLs, authentication, cookies, and artwork URLs",
        },
    }


def level_word(value: Any, low: str, middle: str, high: str) -> str:
    if not isinstance(value, (int, float)):
        return middle
    if value < 0.34:
        return low
    if value > 0.66:
        return high
    return middle


def instrument_roles(instruments: Iterable[str]) -> tuple[list[str], list[str]]:
    roles: list[str] = []
    excluded: list[str] = []
    for instrument in instruments:
        normalized = instrument.casefold()
        if normalized in VOCAL_LIKE:
            excluded.append(instrument)
            continue
        role = ROLE_GUIDANCE.get(normalized, instrument)
        if role not in roles:
            roles.append(role)
    return roles, excluded


def build_prompt(
    profile: dict[str, Any],
    *,
    activity: str,
    duration_seconds: int = 180,
    avoid: Iterable[str] = (),
) -> dict[str, Any]:
    genres = profile.get("genres") or ["Electronic"]
    subgenres = profile.get("subgenres") or ["Ambient"]
    moods = profile.get("moods") or ["Focused", "Uplifting"]
    roles, excluded_instruments = instrument_roles(profile.get("instruments", []))
    bpm = profile.get("bpm") or 108
    brightness = level_word(profile.get("brightness"), "dark and warm", "balanced and warm", "bright and open")
    complexity = level_word(profile.get("complexity"), "sparse", "moderately layered", "densely evolving")
    avoid_values = unique_sorted([*avoid, "vocals", "singing", "speech", "spoken word", "chanting", "whispers"])
    if excluded_instruments:
        avoid_values.extend(excluded_instruments)
        avoid_values = unique_sorted(avoid_values)

    positive = (
        f"[Instrumental] Create a {duration_seconds // 60}-minute premium instrumental track for {activity.lower()} and sustained focus. "
        f"Genre: {genres[0]}; subgenres: {', '.join(subgenres)}. "
        f"Mood: {', '.join(moods)}. Tempo: {bpm} BPM in a steady 4/4 meter. "
        f"Use {', '.join(roles)}. "
        f"Keep the tonal character {brightness}, the arrangement {complexity}, and the mix clear, deep, and mature. "
        "Start with a restrained tonal introduction under 10 seconds, establish the main pulse quickly, "
        "then develop contrasting 16-to-32-bar phrases with small changes in harmony, register, countermelody, "
        "and texture. Build energy through musical development rather than a drop. End with a resolved, "
        "crossfade-friendly outro and never repeat one short loop unchanged."
    )
    negative = ", ".join(
        [
            *avoid_values,
            "piercing high frequencies",
            "sub-bass rumble",
            "toy instruments",
            "childish melodies",
            "random notes",
            "white noise",
            "jazz swing",
            "10-second loop repetition",
            "dramatic riser",
            "breakdown",
            "sudden silence",
        ]
    )

    return {
        "schema": "aria-focus.music-prompt",
        "schema_version": 1,
        "profile_id": profile["profile_id"],
        "selection": {
            "activity": activity,
            "genre": genres[0],
            "subgenres": subgenres,
            "moods": moods,
            "instruments": profile.get("instruments", []),
            "excluded_instruments": excluded_instruments,
            "bpm": bpm,
            "brightness": profile.get("brightness"),
            "complexity": profile.get("complexity"),
        },
        "prompt": {"positive": positive, "negative": negative},
        "policy": {
            "instrumental_only": True,
            "intro_max_seconds": 10,
            "variation_window_bars": [16, 32],
            "allow_named_artists_or_existing_works": False,
        },
    }


def write_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--har", type=Path, required=True, help="input browser HAR")
    parser.add_argument("--output", type=Path, required=True, help="sanitized catalog JSON")
    parser.add_argument("--example-output", type=Path, required=True, help="one generated prompt JSON")
    parser.add_argument("--profile-id", default="profile-001")
    parser.add_argument("--activity", default="Motivation")
    parser.add_argument("--generated-on", default="2026-07-22")
    args = parser.parse_args()

    catalog = derive_catalog(args.har, args.generated_on)
    profile = next(
        (item for item in catalog["profiles"] if item["profile_id"] == args.profile_id),
        None,
    )
    if profile is None:
        raise SystemExit(f"unknown profile id: {args.profile_id}")

    example = build_prompt(
        profile,
        activity=args.activity,
        avoid=("jazz", "swing", "blues", "funk"),
    )
    write_json(args.output, catalog)
    write_json(args.example_output, example)
    print(
        json.dumps(
            {
                "catalog": str(args.output),
                "profiles": catalog["profile_count"],
                "example": str(args.example_output),
                "profile_id": args.profile_id,
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
