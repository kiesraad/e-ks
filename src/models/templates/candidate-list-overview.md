+++
title = "Overzicht kandidatenlijsten"
language = "nl"
footer_right = "Pagina {page} van {total}"
+++

## Overzicht van de ingeleverde kandidatenlijsten

# Centraal Stembureau

Voor de verkiezing van de leden van **{{ election_name|line }}** op **{{ election_date|line }}**

De voorzitter van het centraal stembureau voor de verkiezing van de leden van **{{ election_name | line}}**

|  |  | Type |
{%- for district in electoral_districts -%} {{ district.region_number() }} |
{%- endfor %}
| --- | --- | --- | {% for _ in electoral_districts %} --- | {% endfor %}
{% for group in groups -%}
  | **{{ group.number }}** | **{{ group.appellation }}** |
  {%- match group.sets.affiliation_type() -%}
    {%- when AffiliationType::SetOfEqualLists -%}
      *G*
    {%- when AffiliationType::GroupOfLists -%}
      *NG*
    {%- when AffiliationType::StandAloneList -%}
      *OZ*
  {%- endmatch -%} |
    {%- for district in electoral_districts -%}
      {%- if group.sets.contains(district) -%}
        \*|
      {%- else -%}
        |
      {%- endif -%}
    {%- endfor %}
{% endfor %}

#### Lijsttypes

Bij het weergeven van de lijsten wordt onderscheid gemaakt tussen de volgende lijsttypes:

- *G:* Lijstengroep (gelijkluidende lijsten)
- *NG:* Lijstengroep (niet gelijkluidende lijsten)
- *OZ:* Op zichzelf staande lijst

#### Kieskringen

Kieskringen en gemeente of openbaar lichaam waar hoofdstembureau is gevestigd:
{% for district in electoral_districts %}
{{ district.region_number() }}. {{ district.title() }}
{%- endfor %}

@pagebreak
{% for group in groups %}
#### {{ group.number }}. {{ group.appellation }}

{ widths = "auto 1" }
| Stel | Kieskringen |
| --- | --- |
{% for (number, districts) in group.stels() -%}
| {% if let Some(number) = number %}**{{ number }}**{% endif %} | **{{ districts }}** |
{% endfor %}
{% endfor %}
