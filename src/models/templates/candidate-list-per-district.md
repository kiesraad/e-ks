+++
title = "Kandidatenlijsten"
language = "nl"
footer_right = "Pagina {page} van {total}"
+++

# Overzicht definitieve kandidatenlijsten
Voor de verkiezing van: **{{ election_name|line }}**
Dag van stemming: **{{ election_date|line }}**

Kieskring: **{{ electoral_district|line }}**

{% for numbered in lists %}
## {{ numbered.number }} {{ numbered.list.appellation|line }}
{ widths = "auto 2 1" }
| Nr | naam | woonplaats |
| --- | --- | --- |
{%- for candidate in numbered.list.candidates %}
| {{ candidate.position }} | {{ candidate.name|cell }} | {{ candidate.locality|cell }} |
{%- endfor %}
{% if !loop.last %}
@pagebreak
{% endif %}
{% endfor %}
