#!/bin/sh
for dataset in companies campaigns ads; do
  curl -O https://examples.citusdata.com/tutorial/${dataset}.csv
done
