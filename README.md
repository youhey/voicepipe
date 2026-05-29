# voicepipe

Personal Voice Rendering Pipeline for Radio Scripts.

A tiny pipeline that turns radio-style scripts into narrated audio programs.

voicepipe is the downstream renderer of the digestpipe → radiopipe pipeline.

It consumes structured radio scripts and produces narrated audio episodes using local text-to-speech engines such as VOICEVOX.

## Concept

Information
↓
digestpipe

Structured News Digest
↓
radiopipe

Radio Narration Script
↓
voicepipe

Narrated Audio Program

## What is voicepipe?

voicepipe is a Radio Narration Rendering pipeline.

It transforms generated radio scripts into narrated audio programs by combining text-to-speech synthesis and audio rendering.
