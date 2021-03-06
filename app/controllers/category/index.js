import { readOnly } from '@ember/object/computed';
import Controller from '@ember/controller';
import { computed } from '@ember/object';

import PaginationMixin from '../../mixins/pagination';

export default Controller.extend(PaginationMixin, {
    queryParams: ['page', 'per_page', 'sort'],
    page: '1',
    per_page: 10,
    sort: 'recent-downloads',

    totalItems: readOnly('model.meta.total'),

    category: null,

    currentSortBy: computed('sort', function() {
        if (this.get('sort') === 'downloads') {
            return 'All-Time Downloads';
        } else if (this.get('sort') === 'alpha') {
            return 'Alphabetical';
        } else {
            return 'Recent Downloads';
        }
    }),
});
