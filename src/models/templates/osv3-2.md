+++
title = "Publicatie kandidatenlijsten"
language = "nl"
footer_right = "Pagina {page} van {total}"
+++

# Centraal Stembureau

Kandidatenlijsten verkiezing van de leden van *{{ election_name|line }}*

De voorzitter van het centraal stembureau voor verkiezing van de leden van **{{ election_name|line }}**;

gelet op artikel S 13 van de Kieswet;

maakt bekend dat voor de op **{{ election_date|line }}** te houden verkiezing de volgende geldige kandidatenlijsten zijn ingeleverd:

{% for district in valid_lists %}
## Kieskring {{ district.electoral_district|line }}

{% for numbered in district.lists %}
{ numbered = false }
### Lijst {{ numbered.number }}. {{ numbered.list.appellation|line }}

{ widths = "auto 2 1" }
|  | naam | woonplaats |
| --- | --- | --- |
{%- for candidate in numbered.list.candidates %}
| {{ candidate.position }} | {{ candidate.name|cell }} | {{ candidate.locality|cell }} |
{%- endfor %}

{% if !loop.last %}
@pagebreak
{% endif %}
{% endfor %}

{% if !loop.last %}
@pagebreak
{% endif %}
{% endfor %}
