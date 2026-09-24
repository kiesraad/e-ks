+++
title = "Overzicht controle voorinlevering"
language = "nl"
header_right = "Overzicht controle voorinlevering"
footer_right = "Pagina {page} van {total}"
+++

# Overzicht controle voorinlevering

{ widths = "2 5" }
| Verkiezing | {{ election_name|cell }} |
| Aanduiding | {{ appellation|cell }} |
| Datum | {{ date|long_date|cell }} |
| Kandidaten zonder BRP fouten | {{ candidates_without_brp_errors }} |
| Kandidaten met BRP fouten | {{ candidates_with_brp_errors }} |
| Totaal aantal BRP fouten | {{ brp_error_count }} |
| Totaal aantal aandachtspunten | {{ problem_count }} |

{% if !complete %}
{ background = "highlight" }
> **Let op!**\
> Nog niet alle kandidaten zijn gecontroleerd tegen de BRP. Dit overzicht kan
> daardoor onvolledig zijn.
{% endif %}

{% if candidates.is_empty() %}
Er zijn geen verschillen met de BRP en geen aandachtspunten gevonden.
{% else %}
{% for candidate in candidates %}
#### {{ candidate.heading()|line }}

{ widths = "1 2" }
{%- for detail in candidate.details %}
| {{ detail.label|cell }} | {{ detail.value|cell }} |
{%- endfor %}

{% if !candidate.findings.is_empty() %}
Verschillen met de BRP:

{% for finding in candidate.findings %}
- {{ finding }}
{%- endfor %}
{% endif %}
{% if !candidate.problems.is_empty() %}
Aandachtspunten bij de kandidatenlijst:

{% for problem in candidate.problems %}
- {{ problem }}
{%- endfor %}
{% endif %}
{% endfor %}
{% endif %}
