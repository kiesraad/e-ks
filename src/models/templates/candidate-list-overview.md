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
{% for (political_group, district_batches) in lists -%}
  | {{ loop.index }} | {{ political_group }} |
  {%- match self.affiliation_type(&district_batches) -%}
    {%- when AffiliationType::SetOfEqualLists -%}
      G
    {%- when AffiliationType::GroupOfLists -%}
      NG
    {%- when AffiliationType::StandAloneList -%}
      OZ
  {%- endmatch -%} |
  
  {%- let active_districts = self.active_districts(&district_batches) %}
    {%- for district in electoral_districts -%}
      {%- if active_districts.contains(&district) -%}
        \*|
      {%- else -%}
        |
      {%- endif -%}
    {%- endfor %}
{% endfor %}

#### Lijsttypes

Bij het weergeven van de lijsten wordt onderscheid gemaakt tussen de volgende lijsttypes:

- *G*: Lijstengroep (gelijkluidende lijsten)
- *NG*: Lijstengroep (niet gelijkluidende lijsten)
- *OZ*: Op zichzelfstaande lijst

#### Kieskringen

Kieskringen en gemeente of openbaar lichaam waar hoofdstembureau is gevestigd:
{% for district in electoral_districts %}
{{ district.region_number() }}. {{ district.title() }}
{%- endfor %}

@pagebreak
{% for (political_group, district_batches) in lists %}
#### {{ loop.index }}. {{ political_group }}

| Stel | Kieskringen |
| --- | --- |
{% for (number, districts) in self.number_batched_districts(&district_batches) -%}
| {{ number | assigned_or("") }} | {{ districts }} |
{% endfor %}
{% endfor %}
