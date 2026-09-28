+++
title = "Verzuimbrief"
language = "nl"
header_right = "Verzuimbrief"
footer_right = "Pagina {page} van {total}"
+++

{% macro bullets(omissions) %}
{%- for omission in omissions.iter() %}
- {{ omission.description }}{% if let Some(help_text) = omission.help_text %}\
  {{ help_text }}{% endif %}
{%- endfor %}
{% endmacro %}

AANTEKENEN\
{{ addressee.initials }} {{ addressee.last_name }}\
{{ addressee.postal_address.street_address }}\
{{ addressee.postal_address.postal_code }} {{ addressee.postal_address.locality }}

@spacer(3em)

{{ location }}, {{ date|long_date }}

#### Onderwerp

Verzuim kandidatenlijst voor de verkiezing van {{ election_name }}

@spacer(3em)

Geachte lijstinleveraar,

{% if omission_groups.is_empty() %}
Hierbij deel ik u mede dat het centraal stembureau voor de verkiezing van
{{ election_name }} in zijn vergadering van heden geen verzuimen heeft
geconstateerd met betrekking tot de aanduiding **{{ appellation }}.**
{% else %}
Hierbij deel ik u mede dat het centraal stembureau voor de verkiezing van
{{ election_name }} in zijn vergadering van heden een of meerdere verzuimen
heeft geconstateerd met betrekking tot de aanduiding **{{ appellation }}.**
{% endif %}

{% for group in omission_groups %}
#### {{ group.heading()|line }}

Verzuim:

{% call bullets(group.omissions) %}{% endcall %}
{% for candidate in group.candidates %}
##### {{ candidate.heading()|line }}

{% call bullets(candidate.omissions) %}{% endcall %}
{% endfor %}
{% endfor %}
{% if !omission_groups.is_empty() %}
Deze verzuimen kunnen door de inleveraar van de kandidatenlijst of door één van
de op de kandidatenlijst vermelde vervangers worden hersteld tot
**{{ recovery_deadline_date|long_date }}, {{ recovery_deadline_time }} uur** op
{{ recovery_address }}.
{% endif %}

Hoogachtend,

namens het centraal stembureau,

@spacer(4em)

{ widths = "1 1" }
| {{ chair|cell }} | {{ secretary|cell }} |
| voorzitter | secretaris-directeur |

{% if !declarations_of_support.is_empty() %}
@pagebreak

#### Bijlage 1: overzicht aantallen ondersteuningsverklaringen per kieskring

Aanduiding: **{{ appellation }}**

{ widths = "2 1 1 1" }
| kieskring | aantal ingeleverde verklaringen | aantal goedgekeurde verklaringen | minimaal aantal nog aan te leveren verklaringen |
| --- | ---: | ---: | ---: |
{%- for row in declarations_of_support %}
| {{ row.electoral_district|cell }} | {{ row.submitted|or_dash|cell }} | {{ row.approved|or_dash|cell }} | {{ row.still_required|or_dash|cell }} |
{%- endfor %}

Indien is geconstateerd dat voor een kieskring voldoende geldige
ondersteuningsverklaringen zijn overlegd, is veelal gestopt met het controleren
van de overige ondersteuningsverklaringen in de betreffende kieskring.

{ background = "highlight" }
> **Let op!**\
> Ondanks een zorgvuldige samenstelling van deze bijlage kunnen aan deze
> aantallen geen rechten worden ontleend. Indien u zeker wilt zijn van de
> geconstateerde aantallen, kunt u de ondersteuningsverklaringen inzien op
> {{ recovery_address }}.
{% endif %}
