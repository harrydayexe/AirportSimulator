{{/* The chart's name. */}}
{{- define "airport-simulator.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
The name everything is called. Pods are <fullname>-0, <fullname>-1, ...,
and the number after the last "-" is how each replica learns which it is.
*/}}
{{- define "airport-simulator.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 52 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 52 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 52 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/* The headless Service that gives each replica its own DNS name. */}}
{{- define "airport-simulator.peerService" -}}
{{- printf "%s-peer" (include "airport-simulator.fullname" .) }}
{{- end }}

{{- define "airport-simulator.selectorLabels" -}}
app.kubernetes.io/name: {{ include "airport-simulator.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{- define "airport-simulator.labels" -}}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{ include "airport-simulator.selectorLabels" . }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Every replica and where to reach it, as airport-server's --peers wants it:
1=<pod-0 address>:<port>,2=<pod-1 address>:<port>,...
Replica N is pod N-1.
*/}}
{{- define "airport-simulator.peers" -}}
{{- $fullname := include "airport-simulator.fullname" . -}}
{{- $service := include "airport-simulator.peerService" . -}}
{{- $peers := list -}}
{{- range $i := until (int .Values.replicaCount) -}}
{{- $address := printf "%s-%d.%s.%s.svc.%s" $fullname $i $service $.Release.Namespace $.Values.clusterDomain -}}
{{- $peers = append $peers (printf "%d=%s:%d" (add1 $i) $address (int $.Values.ports.peer)) -}}
{{- end -}}
{{- join "," $peers -}}
{{- end }}
