# Evaluation Harness

## Overview

`markkit-eval` measures extraction quality using LLM-as-judge
to compare markkit output against reference extractions.

## Methodology

1. Curate a dataset of (URL, reference markdown) pairs
2. Extract markdown from each URL using markkit
3. Score extraction quality via LLM comparison
4. Track metrics over time

## Metrics

- **Completeness**: Is all important content preserved?
- **Accuracy**: Is the content faithful to the original?
- **Formatting**: Is the markdown well-structured?
- **Noise**: Is non-content (nav, ads, etc.) excluded?

## Providers

- Anthropic (Claude)
- OpenAI (GPT)
